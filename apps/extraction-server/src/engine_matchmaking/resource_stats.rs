use super::*;

impl EngineMatchWorldRuntime {
    pub(crate) fn new(
        server: Addr<Server>,
        matchmaking: Weak<MatchmakingService>,
        catalog: Arc<EngineCatalog>,
    ) -> Self {
        Self {
            server,
            matchmaking,
            generations: Arc::new(Mutex::new(HashMap::new())),
            owned_matches: Mutex::new(HashMap::new()),
            forced_eliminations: Mutex::new(HashMap::new()),
            hard_deadlines: Mutex::new(HashMap::new()),
            catalog,
        }
    }

    pub(super) fn diagnostic_snapshot(&self) -> MatchWorldRuntimeResourceSnapshot {
        MatchWorldRuntimeResourceSnapshot {
            generations: self
                .generations
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .len(),
            owned_matches: self
                .owned_matches
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .len(),
            forced_eliminations: self
                .forced_eliminations
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .len(),
            hard_deadlines: self
                .hard_deadlines
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use actix::Actor;

    use super::*;

    #[actix::test]
    async fn snapshot_reads_the_actual_runtime_maps() {
        let server = Server::new().debug(false).build().start();
        let catalog = Arc::new(
            EngineCatalog::from_manifest(&crate::contracts::bundled_manifest().unwrap()).unwrap(),
        );
        let runtime = EngineMatchWorldRuntime::new(server, Weak::new(), catalog);
        runtime
            .generations
            .lock()
            .unwrap()
            .insert("world".to_owned(), "generation".to_owned());
        runtime
            .owned_matches
            .lock()
            .unwrap()
            .insert("world".to_owned(), Uuid::new_v4());
        runtime
            .forced_eliminations
            .lock()
            .unwrap()
            .insert("world".to_owned(), ForcedEliminationQueue::default());
        runtime
            .hard_deadlines
            .lock()
            .unwrap()
            .insert("world".to_owned(), HardDeadlineControl::default());

        assert_eq!(
            runtime.diagnostic_snapshot(),
            MatchWorldRuntimeResourceSnapshot {
                generations: 1,
                owned_matches: 1,
                forced_eliminations: 1,
                hard_deadlines: 1,
            }
        );
    }
}
