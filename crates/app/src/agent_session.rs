//! AgentSession (阶段 E): owns the vendored-pi RPC process — spawn /
//! epoch / event-receiver lifecycle (ARCHITECTURE.md §3: RPC 会话归
//! AgentSession). Plain struct for now; becomes a headless entity when
//! panels subscribe to session events.

use std::path::Path;

use futures::channel::mpsc::UnboundedReceiver;

use pi_link::client::{spawn as spawn_pi, PiSession};
use pi_link::protocol::Event;

pub(crate) struct AgentSession {
    pub session: Option<PiSession>,
    pub epoch: u64,
}

impl AgentSession {
    pub(crate) fn new(epoch: u64) -> Self {
        Self {
            session: None,
            epoch,
        }
    }

    /// Spawn with raw CLI args (per-session tool presets etc. — the RPC
    /// surface has no live tool switching, so tools ride spawn flags).
    pub(crate) fn spawn_with(
        &mut self,
        cwd: &Path,
        extra_args: &[String],
    ) -> Option<UnboundedReceiver<Event>> {
        self.epoch += 1;
        let args: Vec<&str> = extra_args.iter().map(String::as_str).collect();
        match spawn_pi(cwd, &args) {
            Ok((s, ev)) => {
                self.session = Some(s);
                Some(ev)
            }
            Err(e) => {
                eprintln!("{e}");
                self.session = None;
                None
            }
        }
    }
}
