use std::{net::SocketAddr, sync::Arc, time::Duration};

use crate::{
    config::RelayConfig,
    dns::{HickoryResolver, Resolver},
};

pub(super) struct Propagation {
    pub resolver: Arc<dyn Resolver>,
    pub timeout: Duration,
    pub interval: Duration,
    pub query_timeout: Duration,
    pub update_timeout: Duration,
    pub cleanup_timeout: Duration,
    pub attempt_timeout: Duration,
}

impl Propagation {
    pub fn from_config(cfg: &RelayConfig) -> anyhow::Result<Self> {
        let dns = &cfg.dns01;
        for (name, seconds) in [
            ("propagation_timeout_secs", dns.propagation_timeout_secs),
            ("query_timeout_secs", dns.query_timeout_secs),
            ("cleanup_timeout_secs", dns.cleanup_timeout_secs),
            ("attempt_timeout_secs", dns.attempt_timeout_secs),
        ] {
            if !(1..=86400).contains(&seconds) {
                anyhow::bail!("signer.relay.dns01.{name} must be 1 through 86400");
            }
        }
        if !(1..=60000).contains(&dns.propagation_interval_ms) {
            anyhow::bail!("signer.relay.dns01.propagation_interval_ms must be 1 through 60000");
        }
        let required = dns
            .rfc2136
            .timeout_secs
            .checked_add(dns.propagation_timeout_secs)
            .and_then(|value| value.checked_add(cfg.poll_timeout_secs))
            .and_then(|value| value.checked_add(dns.cleanup_timeout_secs));
        if required.is_none_or(|value| value >= dns.attempt_timeout_secs) {
            anyhow::bail!(
                "dns01.attempt_timeout_secs must be more than the combined UPDATE, propagation, validation, and cleanup limits"
            );
        }
        if dns.cleanup_timeout_secs < dns.rfc2136.timeout_secs {
            anyhow::bail!("dns01.cleanup_timeout_secs must cover rfc2136.timeout_secs");
        }
        let addr: SocketAddr = dns.propagation_resolver.parse().map_err(|_| {
            anyhow::anyhow!("dns01.propagation_resolver must be an IP address and port")
        })?;
        if dns.propagation_resolver == dns.rfc2136.server {
            anyhow::bail!("dns01.propagation_resolver must be different from the UPDATE server");
        }
        Ok(Self {
            resolver: Arc::new(HickoryResolver::from_address_uncached(addr)?),
            timeout: Duration::from_secs(dns.propagation_timeout_secs),
            interval: Duration::from_millis(dns.propagation_interval_ms),
            query_timeout: Duration::from_secs(dns.query_timeout_secs),
            update_timeout: Duration::from_secs(dns.rfc2136.timeout_secs),
            cleanup_timeout: Duration::from_secs(dns.cleanup_timeout_secs),
            attempt_timeout: Duration::from_secs(dns.attempt_timeout_secs),
        })
    }

    pub async fn wait(&self, name: &str, value: &str) -> Result<(), String> {
        tokio::time::timeout(self.timeout, async {
            loop {
                if let Ok(Ok(values)) =
                    tokio::time::timeout(self.query_timeout, self.resolver.txt(name)).await
                    && values
                        .iter()
                        .any(|answer| answer.as_bytes() == value.as_bytes())
                {
                    return;
                }
                tokio::time::sleep(self.interval).await;
            }
        })
        .await
        .map_err(|_| "public DNS propagation timed out".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::{collections::VecDeque, net::IpAddr, sync::Mutex};

    struct Answers(Mutex<VecDeque<Result<Vec<String>, String>>>);
    #[async_trait]
    impl Resolver for Answers {
        async fn reverse(&self, _: IpAddr) -> Result<Vec<String>, String> {
            unreachable!()
        }
        async fn forward(&self, _: &str) -> Result<Vec<IpAddr>, String> {
            unreachable!()
        }
        async fn txt(&self, _: &str) -> Result<Vec<String>, String> {
            self.0
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(vec![]))
        }
    }

    #[tokio::test]
    async fn propagation_retries_errors_and_preserves_case() {
        let resolver = Arc::new(Answers(Mutex::new(VecDeque::from([
            Err("resolver failure".into()),
            Ok(vec![]),
            Ok(vec!["VALUE".into()]),
            Ok(vec!["other".into(), "VaLuE".into()]),
        ]))));
        let settings = Propagation {
            resolver: resolver.clone(),
            interval: Duration::from_millis(1),
            timeout: Duration::from_secs(1),
            query_timeout: Duration::from_millis(10),
            update_timeout: Duration::from_millis(10),
            cleanup_timeout: Duration::from_millis(20),
            attempt_timeout: Duration::from_secs(2),
        };
        settings
            .wait("_acme-challenge.example.org.", "VaLuE")
            .await
            .unwrap();
        assert!(resolver.0.lock().unwrap().is_empty());
    }

    #[test]
    fn configuration_rejects_incompatible_budgets_and_resolvers() {
        for case in 0..9 {
            let mut cfg = RelayConfig::default();
            match case {
                1 => cfg.dns01.propagation_timeout_secs = 0,
                2 => cfg.dns01.query_timeout_secs = 86401,
                3 => cfg.dns01.propagation_interval_ms = 0,
                4 => cfg.dns01.attempt_timeout_secs = 600,
                5 => cfg.dns01.cleanup_timeout_secs = 1,
                6 => cfg.dns01.propagation_resolver = "hostname".into(),
                7 => cfg.dns01.rfc2136.server = cfg.dns01.propagation_resolver.clone(),
                8 => cfg.poll_timeout_secs = u64::MAX,
                _ => {}
            }
            assert_eq!(
                Propagation::from_config(&cfg).is_ok(),
                case == 0,
                "case {case}"
            );
        }
    }
}
