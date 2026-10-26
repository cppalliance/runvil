//! Per-run resource ceilings: [`RunLimits`].

use std::num::{NonZero, NonZeroU32, NonZeroU64, NonZeroUsize};
use std::time::Duration;

/// Resource limits for one run.
///
/// The limits cover tool iterations per `models.loop` or model handle
/// `loop` call, task concurrency, model response size, Lua memory, Lua log
/// volume, and how long a model request waits for its response.
///
/// The defaults are built-in constants and are safe to use as they are. A
/// prompt's frontmatter `max_tool_iterations` field, when present, overrides
/// [`RunLimits::max_tool_iterations`] for that prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct RunLimits {
    pub(crate) max_tool_iterations: NonZeroU32,
    pub(crate) max_concurrency: NonZeroUsize,
    pub(crate) max_response_bytes: NonZeroU64,
    pub(crate) lua_memory_bytes: NonZeroUsize,
    pub(crate) lua_log_events: NonZeroU32,
    pub(crate) request_timeout: Duration,
}

impl RunLimits {
    /// Creates limits with the default values.
    ///
    /// The defaults are 24 tool iterations per `models.loop` or model
    /// handle `loop` call, 8 concurrent tasks, a 16 MiB model response cap,
    /// 64 MiB of Lua memory, 1024 Lua log events, and a 120 second wait for
    /// each part of a model response.
    #[must_use]
    pub fn new() -> RunLimits {
        RunLimits {
            max_tool_iterations: const { NonZero::new(24).unwrap() },
            max_concurrency: const { NonZero::new(8).unwrap() },
            max_response_bytes: const { NonZero::new(16 * 1024 * 1024).unwrap() },
            lua_memory_bytes: const { NonZero::new(64 * 1024 * 1024).unwrap() },
            lua_log_events: const { NonZero::new(1024).unwrap() },
            request_timeout: Duration::from_secs(120),
        }
    }

    /// Sets the default maximum number of tool iterations per `models.loop`
    /// or model handle `loop` call.
    ///
    /// A tool iteration is one model round of such a call; `models.infer`
    /// rounds do not count. A prompt's frontmatter `max_tool_iterations`
    /// field overrides this default for that prompt.
    #[must_use]
    pub fn max_tool_iterations(mut self, value: NonZeroU32) -> RunLimits {
        self.max_tool_iterations = value;
        self
    }

    /// Sets the run's concurrency ceiling: the most tasks the scheduler
    /// admits at once across the whole run.
    ///
    /// Each spawned task counts against its owner's limit and against every
    /// ancestor's limit, so limits nest. A fanout started inside one arm of
    /// another fanout runs within that arm's remaining share.
    #[must_use]
    pub fn max_concurrency(mut self, value: NonZeroUsize) -> RunLimits {
        self.max_concurrency = value;
        self
    }

    /// Sets the maximum accepted model response body size, in bytes.
    #[must_use]
    pub fn max_response_bytes(mut self, value: NonZeroU64) -> RunLimits {
        self.max_response_bytes = value;
        self
    }

    /// Sets the memory ceiling for each Lua virtual machine, in bytes.
    #[must_use]
    pub fn lua_memory_bytes(mut self, value: NonZeroUsize) -> RunLimits {
        self.lua_memory_bytes = value;
        self
    }

    /// Sets the maximum number of `log` events that a prompt's Lua code can
    /// record in each Lua virtual machine.
    #[must_use]
    pub fn lua_log_events(mut self, value: NonZeroU32) -> RunLimits {
        self.lua_log_events = value;
        self
    }

    /// Sets the longest time a model request waits for the next part of its
    /// response.
    ///
    /// The request waits first for the response headers, then for each body
    /// chunk. Each arrival restarts the wait, so a long stream runs for as
    /// long as its parts keep arriving.
    #[must_use]
    pub fn request_timeout(mut self, value: Duration) -> RunLimits {
        self.request_timeout = value;
        self
    }

    /// Returns the maximum accepted model response body size, in bytes.
    #[must_use]
    pub fn response_bytes(&self) -> NonZeroU64 {
        self.max_response_bytes
    }

    /// Returns the longest time a model request waits for the next part of
    /// its response.
    #[must_use]
    pub fn timeout(&self) -> Duration {
        self.request_timeout
    }
}

impl Default for RunLimits {
    fn default() -> RunLimits {
        RunLimits::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_limits_pins_all_six_defaults_and_the_untested_builders() {
        let defaults = RunLimits::new();
        assert_eq!(defaults.max_tool_iterations.get(), 24);
        assert_eq!(defaults.max_concurrency.get(), 8);
        assert_eq!(defaults.response_bytes().get(), 16 * 1024 * 1024);
        assert_eq!(defaults.lua_memory_bytes.get(), 64 * 1024 * 1024);
        assert_eq!(defaults.lua_log_events.get(), 1024);
        assert_eq!(defaults.timeout(), Duration::from_secs(120));

        let built = RunLimits::new()
            .max_response_bytes(const { NonZero::new(4 * 1024).unwrap() })
            .lua_log_events(const { NonZero::new(7).unwrap() })
            .request_timeout(Duration::from_secs(5));
        assert_eq!(built.response_bytes().get(), 4 * 1024);
        assert_eq!(built.lua_log_events.get(), 7);
        assert_eq!(built.timeout(), Duration::from_secs(5));
    }

    #[test]
    fn the_tool_iteration_builder_replaces_the_default_limit() {
        let limits = RunLimits::new().max_tool_iterations(const { NonZero::new(8).unwrap() });
        assert_eq!(limits.max_tool_iterations.get(), 8);
    }

    #[test]
    fn limits_compare_and_hash_by_value() {
        let tighter = RunLimits::new().max_tool_iterations(const { NonZero::new(8).unwrap() });
        assert_eq!(RunLimits::new(), RunLimits::default());
        assert_ne!(RunLimits::new(), tighter);
        let distinct: std::collections::HashSet<RunLimits> =
            [RunLimits::new(), RunLimits::default(), tighter]
                .into_iter()
                .collect();
        assert_eq!(distinct.len(), 2, "equal limits hash alike");
    }
}
