use super::*;
use crate::sqlite::status::OrderStatus;

/// A `DnsUpdater` that records what it was asked to publish, so a test can
/// assert on the record without a DNS server.
#[derive(Default)]
struct StubUpdater {
    published: std::sync::Mutex<Vec<(String, String)>>,
    deleted: std::sync::Mutex<Vec<(String, String)>>,
    fail: bool,
    hidden: std::sync::atomic::AtomicBool,
    queries: std::sync::atomic::AtomicUsize,
    cleanup_fail: bool,
}

#[async_trait]
impl dns01::DnsUpdater for StubUpdater {
    async fn upsert_txt(&self, name: &str, value: &str) -> Result<(), String> {
        if self.fail {
            return Err("no DNS for you".to_string());
        }
        self.published
            .lock()
            .unwrap()
            .push((name.to_string(), value.to_string()));
        Ok(())
    }
    async fn delete_txt(&self, name: &str, value: &str) -> Result<(), String> {
        self.deleted
            .lock()
            .unwrap()
            .push((name.to_string(), value.to_string()));
        if self.cleanup_fail {
            return Err("cleanup failed".into());
        }
        Ok(())
    }
}

#[async_trait]
impl crate::dns::Resolver for StubUpdater {
    async fn reverse(&self, _: std::net::IpAddr) -> Result<Vec<String>, String> {
        unreachable!()
    }
    async fn forward(&self, _: &str) -> Result<Vec<std::net::IpAddr>, String> {
        unreachable!()
    }
    async fn txt(&self, name: &str) -> Result<Vec<String>, String> {
        self.queries
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if self.hidden.load(std::sync::atomic::Ordering::SeqCst) {
            return Ok(vec!["unrelated-value".into()]);
        }
        Ok(self
            .published
            .lock()
            .unwrap()
            .iter()
            .filter(|(owner, _)| owner == name)
            .map(|(_, value)| value.clone())
            .collect())
    }
}

fn propagation(updater: Arc<StubUpdater>) -> super::super::dns01_propagation::Propagation {
    super::super::dns01_propagation::Propagation {
        resolver: updater,
        timeout: Duration::from_millis(200),
        interval: Duration::from_millis(5),
        query_timeout: Duration::from_millis(20),
        update_timeout: Duration::from_millis(100),
        cleanup_timeout: Duration::from_millis(200),
        attempt_timeout: Duration::from_secs(5),
    }
}

/// Swaps the strategy on an already-built signer, so these tests do not
/// need a live RFC 2136 server to exercise the orchestration around it.
fn with_updater(signer: RelaySigner, updater: Arc<StubUpdater>) -> RelaySigner {
    let inner = Arc::try_unwrap(signer.0).unwrap_or_else(|_| panic!("sole owner"));
    RelaySigner(Arc::new(Inner {
        dns01_propagation: Some(propagation(updater.clone())),
        strategy: ChallengeStrategy::Dns01(updater),
        ..inner
    }))
}

/// The `bypass` strategy against an upstream that *does* pose a challenge.
///
/// Bypass does not mean "the upstream asks nothing" — it means this server
/// publishes nothing and simply triggers whatever is offered, which is the
/// right behaviour against an upstream validating by some out-of-band
/// arrangement. Whichever challenge comes first is triggered, without
/// caring about its type.
#[tokio::test(flavor = "multi_thread")]
async fn bypass_triggers_the_offered_challenge() {
    let upstream = testsrv::start(Script {
        chain: real_chain().await,
        pose_challenge: true,
        ..Script::default()
    })
    .await;
    let dir = TempDir::new("upstream");
    let db = database().await;
    // No `with_updater`: the default strategy is bypass.
    let queue = test_queue(db.clone());
    let signer = RelaySigner::from_config(
        &config(&upstream, &dir),
        &relay_parts(db.clone(), no_notifiers(), queue.clone()),
        &crate::signer::CarriedState::new(),
    )
    .unwrap();
    let _runner = TestRunner::start(queue, &signer);
    let order = ready_order(db.clone()).await;

    signer
        .issue(
            order.id.to_string().as_str(),
            &csr_der(),
            &identifiers(),
            RequestedValidity::default(),
        )
        .await
        .unwrap();
    await_status(db, order.id.to_string().as_str(), OrderStatus::Valid).await;

    assert_eq!(
        upstream.challenge_triggered(),
        1,
        "bypass still has to trigger the challenge the upstream posed"
    );
}

/// Bypass is type-agnostic: an `http-01`-only authorization is triggered
/// just the same, where the `dns01` strategy refuses it for lack of a
/// record it could publish.
#[tokio::test(flavor = "multi_thread")]
async fn bypass_triggers_a_challenge_of_any_type() {
    let upstream = testsrv::start(Script {
        chain: real_chain().await,
        pose_challenge: true,
        offer_http01: true,
        ..Script::default()
    })
    .await;
    let dir = TempDir::new("upstream");
    let db = database().await;
    let queue = test_queue(db.clone());
    let signer = RelaySigner::from_config(
        &config(&upstream, &dir),
        &relay_parts(db.clone(), no_notifiers(), queue.clone()),
        &crate::signer::CarriedState::new(),
    )
    .unwrap();
    let _runner = TestRunner::start(queue, &signer);
    let order = ready_order(db.clone()).await;

    signer
        .issue(
            order.id.to_string().as_str(),
            &csr_der(),
            &identifiers(),
            RequestedValidity::default(),
        )
        .await
        .unwrap();
    await_status(db, order.id.to_string().as_str(), OrderStatus::Valid).await;
}

/// A rejected challenge fails the order rather than hanging: under bypass
/// there is nothing to retract, so the only thing to get right is that the
/// failure reaches the local order.
#[tokio::test(flavor = "multi_thread")]
async fn bypass_fails_the_order_when_the_upstream_rejects() {
    let upstream = testsrv::start(Script {
        pose_challenge: true,
        fail_challenge: true,
        ..Script::default()
    })
    .await;
    let dir = TempDir::new("upstream");
    let db = database().await;
    let queue = test_queue(db.clone());
    let signer = RelaySigner::from_config(
        &config(&upstream, &dir),
        &relay_parts(db.clone(), no_notifiers(), queue.clone()),
        &crate::signer::CarriedState::new(),
    )
    .unwrap();
    let _runner = TestRunner::start(queue, &signer);
    let order = ready_order(db.clone()).await;

    signer
        .issue(
            order.id.to_string().as_str(),
            &csr_der(),
            &identifiers(),
            RequestedValidity::default(),
        )
        .await
        .unwrap();
    await_status(db, order.id.to_string().as_str(), OrderStatus::Invalid).await;
}

/// The dns-01 path end to end: publish the record the upstream asked for,
/// trigger it, and clean up afterwards.
#[tokio::test(flavor = "multi_thread")]
async fn dns01_publishes_triggers_and_cleans_up() {
    let upstream = testsrv::start(Script {
        chain: real_chain().await,
        pose_challenge: true,
        ..Script::default()
    })
    .await;
    let dir = TempDir::new("upstream");
    let db = database().await;
    let updater = Arc::new(StubUpdater::default());
    let queue = test_queue(db.clone());
    let signer = with_updater(
        RelaySigner::from_config(
            &config(&upstream, &dir),
            &relay_parts(db.clone(), no_notifiers(), queue.clone()),
            &crate::signer::CarriedState::new(),
        )
        .unwrap(),
        updater.clone(),
    );
    let _runner = TestRunner::start(queue, &signer);
    let order = ready_order(db.clone()).await;

    signer
        .issue(
            order.id.to_string().as_str(),
            &csr_der(),
            &identifiers(),
            RequestedValidity::default(),
        )
        .await
        .unwrap();
    await_status(db, order.id.to_string().as_str(), OrderStatus::Valid).await;

    assert_eq!(
        upstream.challenge_triggered(),
        1,
        "the challenge must be triggered"
    );

    let published = updater.published.lock().unwrap().clone();
    assert_eq!(published.len(), 1);
    let (name, value) = &published[0];
    assert_eq!(name, "_acme-challenge.example.com.");

    // The value must be the digest of a key authorization built from THIS
    // proxy's thumbprint at the upstream — not the end client's, which is
    // the whole reason the client cannot answer this itself.
    let thumbprint = crate::extractors::acme::jwk_thumbprint(signer.0.account.spki_der()).unwrap();
    let expected =
        crate::challenge::dns_01::expected_value(&format!("upstream-token-value.{thumbprint}"));
    assert_eq!(value, &expected);

    // And the record must not be left behind.
    assert_eq!(updater.deleted.lock().unwrap().clone(), published);
}

/// The record must be retracted even when validation fails, so a failed
/// attempt does not litter the zone.
#[tokio::test(flavor = "multi_thread")]
async fn dns01_cleans_up_after_a_rejected_challenge() {
    let upstream = testsrv::start(Script {
        pose_challenge: true,
        fail_challenge: true,
        ..Script::default()
    })
    .await;
    let dir = TempDir::new("upstream");
    let db = database().await;
    let updater = Arc::new(StubUpdater::default());
    let queue = test_queue(db.clone());
    let signer = with_updater(
        RelaySigner::from_config(
            &config(&upstream, &dir),
            &relay_parts(db.clone(), no_notifiers(), queue.clone()),
            &crate::signer::CarriedState::new(),
        )
        .unwrap(),
        updater.clone(),
    );
    let _runner = TestRunner::start(queue, &signer);
    let order = ready_order(db.clone()).await;

    signer
        .issue(
            order.id.to_string().as_str(),
            &csr_der(),
            &identifiers(),
            RequestedValidity::default(),
        )
        .await
        .unwrap();
    await_status(db, order.id.to_string().as_str(), OrderStatus::Invalid).await;

    assert_eq!(
        updater.deleted.lock().unwrap().len(),
        1,
        "a failed attempt must still retract its record"
    );
}

/// An upstream offering no dns-01 cannot be satisfied by this server, and
/// must say so rather than trying a challenge it cannot answer.
#[tokio::test(flavor = "multi_thread")]
async fn dns01_refuses_an_upstream_offering_only_http01() {
    let upstream = testsrv::start(Script {
        pose_challenge: true,
        offer_http01: true,
        ..Script::default()
    })
    .await;
    let dir = TempDir::new("upstream");
    let db = database().await;
    let queue = test_queue(db.clone());
    let signer = with_updater(
        RelaySigner::from_config(
            &config(&upstream, &dir),
            &relay_parts(db.clone(), no_notifiers(), queue.clone()),
            &crate::signer::CarriedState::new(),
        )
        .unwrap(),
        Arc::new(StubUpdater::default()),
    );
    let _runner = TestRunner::start(queue, &signer);
    let order = ready_order(db.clone()).await;

    signer
        .issue(
            order.id.to_string().as_str(),
            &csr_der(),
            &identifiers(),
            RequestedValidity::default(),
        )
        .await
        .unwrap();
    await_status(
        db.clone(),
        order.id.to_string().as_str(),
        OrderStatus::Invalid,
    )
    .await;

    let mapping = UpstreamOrder::find_by_order_id(order.id.to_string().as_str(), &db)
        .await
        .unwrap()
        .unwrap();
    assert!(
        mapping.error.unwrap().contains("no dns-01"),
        "the reason must name what was missing"
    );
    assert_eq!(upstream.challenge_triggered(), 0);
}

/// A DNS provider that cannot publish must fail the order rather than
/// triggering a challenge that is guaranteed to fail.
#[tokio::test(flavor = "multi_thread")]
async fn dns01_fails_when_the_record_cannot_be_published() {
    let upstream = testsrv::start(Script {
        pose_challenge: true,
        ..Script::default()
    })
    .await;
    let dir = TempDir::new("upstream");
    let db = database().await;
    let queue = test_queue(db.clone());
    let signer = with_updater(
        RelaySigner::from_config(
            &config(&upstream, &dir),
            &relay_parts(db.clone(), no_notifiers(), queue.clone()),
            &crate::signer::CarriedState::new(),
        )
        .unwrap(),
        Arc::new(StubUpdater {
            fail: true,
            ..StubUpdater::default()
        }),
    );
    let _runner = TestRunner::start(queue, &signer);
    let order = ready_order(db.clone()).await;

    signer
        .issue(
            order.id.to_string().as_str(),
            &csr_der(),
            &identifiers(),
            RequestedValidity::default(),
        )
        .await
        .unwrap();
    await_status(db, order.id.to_string().as_str(), OrderStatus::Invalid).await;
    assert_eq!(
        upstream.challenge_triggered(),
        0,
        "nothing should be triggered when the record was never published"
    );
}

/// The regression: an upstream offering a challenge type this server does not
/// implement, ahead of the one it does.
///
/// Let's Encrypt began posing `dns-persist-01` beside the three familiar types,
/// and it carries no `token` — which is legal, since its TXT value derives from
/// the account URI. The relay's view of a challenge required one, so serde
/// failed the parse of the **whole** authorization and the `dns-01` challenge
/// next to it was never reached: every relayed order sat `processing` until the
/// client gave up. Nothing here is about `dns-persist-01` in particular; what is
/// asserted is that a type the relay does not answer cannot break an
/// authorization it does not participate in.
#[tokio::test(flavor = "multi_thread")]
async fn dns01_answers_past_a_challenge_type_carrying_no_token() {
    let upstream = testsrv::start(Script {
        chain: real_chain().await,
        pose_challenge: true,
        offer_tokenless_challenge: true,
        ..Script::default()
    })
    .await;
    let dir = TempDir::new("upstream");
    let db = database().await;
    let updater = Arc::new(StubUpdater::default());
    let queue = test_queue(db.clone());
    let signer = with_updater(
        RelaySigner::from_config(
            &config(&upstream, &dir),
            &relay_parts(db.clone(), no_notifiers(), queue.clone()),
            &crate::signer::CarriedState::new(),
        )
        .unwrap(),
        updater.clone(),
    );
    let _runner = TestRunner::start(queue, &signer);
    let order = ready_order(db.clone()).await;

    signer
        .issue(
            order.id.to_string().as_str(),
            &csr_der(),
            &identifiers(),
            RequestedValidity::default(),
        )
        .await
        .unwrap();
    await_status(db, order.id.to_string().as_str(), OrderStatus::Valid).await;

    // The record published must still be the dns-01 one, derived from the
    // token of the challenge the relay actually answered.
    let thumbprint = crate::extractors::acme::jwk_thumbprint(signer.0.account.spki_der()).unwrap();
    let expected = crate::challenge::dns_01::expected_value(&format!(
        "{}.{thumbprint}",
        testsrv::CHALLENGE_TOKEN
    ));
    assert_eq!(
        updater.published.lock().unwrap().clone(),
        vec![("_acme-challenge.example.com.".to_string(), expected)]
    );
    assert_eq!(upstream.challenge_triggered(), 1);
    assert_eq!(
        upstream.tokenless_triggered(),
        0,
        "the relay must never trigger a challenge it has no way to answer"
    );
}

/// Bypass triggers *something*, and with a tokenless type on offer that has to
/// be the one it could actually satisfy — an upstream validating out of band
/// still decides against the challenge it was told to look at.
#[tokio::test(flavor = "multi_thread")]
async fn bypass_prefers_a_challenge_it_could_answer() {
    let upstream = testsrv::start(Script {
        chain: real_chain().await,
        pose_challenge: true,
        offer_tokenless_challenge: true,
        ..Script::default()
    })
    .await;
    let dir = TempDir::new("upstream");
    let db = database().await;
    let queue = test_queue(db.clone());
    let signer = RelaySigner::from_config(
        &config(&upstream, &dir),
        &relay_parts(db.clone(), no_notifiers(), queue.clone()),
        &crate::signer::CarriedState::new(),
    )
    .unwrap();
    let _runner = TestRunner::start(queue, &signer);
    let order = ready_order(db.clone()).await;

    signer
        .issue(
            order.id.to_string().as_str(),
            &csr_der(),
            &identifiers(),
            RequestedValidity::default(),
        )
        .await
        .unwrap();
    await_status(db, order.id.to_string().as_str(), OrderStatus::Valid).await;

    assert_eq!(upstream.challenge_triggered(), 1);
    assert_eq!(upstream.tokenless_triggered(), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn dns01_waits_for_public_dns_before_ca_validation() {
    use std::sync::atomic::Ordering;
    let upstream = testsrv::start(Script {
        chain: real_chain().await,
        pose_challenge: true,
        ..Script::default()
    })
    .await;
    let dir = TempDir::new("propagation");
    let db = database().await;
    let queue = test_queue(db.clone());
    let updater = Arc::new(StubUpdater::default());
    updater.hidden.store(true, Ordering::SeqCst);
    let signer = with_updater(
        RelaySigner::from_config(
            &config(&upstream, &dir),
            &relay_parts(db.clone(), no_notifiers(), queue.clone()),
            &crate::signer::CarriedState::new(),
        )
        .unwrap(),
        updater.clone(),
    );
    let _runner = TestRunner::start(queue, &signer);
    let order = ready_order(db.clone()).await;
    signer
        .issue(
            &order.id.to_string(),
            &csr_der(),
            &identifiers(),
            RequestedValidity::default(),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(2), async {
        while updater.queries.load(Ordering::SeqCst) < 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert_eq!(upstream.challenge_triggered(), 0);
    assert!(updater.deleted.lock().unwrap().is_empty());
    updater.hidden.store(false, Ordering::SeqCst);
    await_status(db, &order.id.to_string(), OrderStatus::Valid).await;
    assert_eq!(upstream.challenge_triggered(), 1);
    assert_eq!(
        *updater.deleted.lock().unwrap(),
        *updater.published.lock().unwrap()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn dns01_propagation_and_outer_deadlines_clean_up_without_validation() {
    for outer_timeout in [false, true] {
        let upstream = testsrv::start(Script {
            pose_challenge: true,
            ..Script::default()
        })
        .await;
        let dir = TempDir::new("propagation-timeout");
        let db = database().await;
        let queue = test_queue(db.clone());
        let updater = Arc::new(StubUpdater::default());
        updater
            .hidden
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let signer = with_updater(
            RelaySigner::from_config(
                &config(&upstream, &dir),
                &relay_parts(db.clone(), no_notifiers(), queue.clone()),
                &crate::signer::CarriedState::new(),
            )
            .unwrap(),
            updater.clone(),
        );
        let mut inner = Arc::try_unwrap(signer.0).ok().unwrap();
        if outer_timeout {
            inner.dns01_propagation.as_mut().unwrap().attempt_timeout = Duration::from_millis(50);
        }
        let signer = RelaySigner(Arc::new(inner));
        let _runner = TestRunner::start(queue, &signer);
        let order = ready_order(db.clone()).await;
        signer
            .issue(
                &order.id.to_string(),
                &csr_der(),
                &identifiers(),
                RequestedValidity::default(),
            )
            .await
            .unwrap();
        await_status(db, &order.id.to_string(), OrderStatus::Invalid).await;
        tokio::time::timeout(Duration::from_secs(1), async {
            while updater.deleted.lock().unwrap().is_empty() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(upstream.challenge_triggered(), 0);
        assert_eq!(
            *updater.deleted.lock().unwrap(),
            *updater.published.lock().unwrap()
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn dns01_cleanup_failure_preserves_ca_failure() {
    let upstream = testsrv::start(Script {
        pose_challenge: true,
        fail_challenge: true,
        ..Script::default()
    })
    .await;
    let dir = TempDir::new("cleanup-failure");
    let db = database().await;
    let queue = test_queue(db.clone());
    let updater = Arc::new(StubUpdater {
        cleanup_fail: true,
        ..StubUpdater::default()
    });
    let signer = with_updater(
        RelaySigner::from_config(
            &config(&upstream, &dir),
            &relay_parts(db.clone(), no_notifiers(), queue.clone()),
            &crate::signer::CarriedState::new(),
        )
        .unwrap(),
        updater.clone(),
    );
    let _runner = TestRunner::start(queue, &signer);
    let order = ready_order(db.clone()).await;
    signer
        .issue(
            &order.id.to_string(),
            &csr_der(),
            &identifiers(),
            RequestedValidity::default(),
        )
        .await
        .unwrap();
    await_status(db.clone(), &order.id.to_string(), OrderStatus::Invalid).await;
    let mapping = UpstreamOrder::find_by_order_id(&order.id.to_string(), &db)
        .await
        .unwrap()
        .unwrap();
    assert!(mapping.error.unwrap().contains("upstream rejected"));
    assert_eq!(updater.deleted.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn cancellation_during_update_waits_for_the_write_before_cleanup() {
    struct SlowUpdater {
        started: tokio::sync::Notify,
        release: tokio::sync::Notify,
        deleted: tokio::sync::Notify,
    }
    #[async_trait]
    impl dns01::DnsUpdater for SlowUpdater {
        async fn upsert_txt(&self, _: &str, _: &str) -> Result<(), String> {
            self.started.notify_one();
            self.release.notified().await;
            Ok(())
        }
        async fn delete_txt(&self, _: &str, value: &str) -> Result<(), String> {
            assert_eq!(value, "attempt-value");
            self.deleted.notify_one();
            Ok(())
        }
    }
    let updater = Arc::new(SlowUpdater {
        started: Default::default(),
        release: Default::default(),
        deleted: Default::default(),
    });
    let worker_updater = updater.clone();
    let task = tokio::spawn(async move {
        super::super::dns01_cleanup::PublishedTxt::publish(
            worker_updater,
            &propagation(Arc::new(StubUpdater::default())),
            "_acme-challenge.example.org.".into(),
            "attempt-value".into(),
        )
        .await
    });
    updater.started.notified().await;
    task.abort();
    let _ = task.await;
    assert!(
        tokio::time::timeout(Duration::from_millis(10), updater.deleted.notified())
            .await
            .is_err()
    );
    updater.release.notify_one();
    tokio::time::timeout(Duration::from_secs(1), updater.deleted.notified())
        .await
        .unwrap();
}
