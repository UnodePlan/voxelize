use super::{WorldConfig, WorldConfigBuilder};

impl WorldConfigBuilder {
    /// Create a world configuration.
    pub fn build(self) -> WorldConfig {
        if self.max_chunk[0] < self.min_chunk[0] || self.max_chunk[1] < self.min_chunk[1] {
            panic!("Min/max chunk options do not make sense.");
        }
        if self.max_height % self.sub_chunks != 0 {
            panic!("Max height should be divisible by sub-chunks.");
        }
        if !self.entity_visible_radius.is_finite() || self.entity_visible_radius < 0.0 {
            panic!("Entity visible radius must be finite and non-negative.");
        }
        if !self.saving && !self.save_dir.is_empty() {
            panic!("Save directory shouldn't be used unless `config.save` is set to true!");
        }

        WorldConfig {
            max_clients: self.max_clients,
            request_policy: self.request_policy,
            chunk_load_policy: self.chunk_load_policy,
            entity_visibility_policy: self.entity_visibility_policy,
            client_disconnect_policy: self.client_disconnect_policy,
            chunk_size: self.chunk_size,
            sub_chunks: self.sub_chunks,
            max_height: self.max_height,
            max_light_level: self.max_light_level,
            max_chunks_per_tick: self.max_chunks_per_tick,
            max_updates_per_tick: self.max_updates_per_tick,
            max_response_per_tick: self.max_response_per_tick,
            max_saves_per_tick: self.max_saves_per_tick,
            time_per_day: self.time_per_day,
            water_level: self.water_level,
            seed: self.seed,
            min_chunk: self.min_chunk,
            max_chunk: self.max_chunk,
            default_time: self.default_time.max(0.0).min(self.time_per_day as f32),
            preload: self.preload,
            preload_radius: self.preload_radius,
            air_drag: self.air_drag,
            fluid_drag: self.fluid_drag,
            fluid_density: self.fluid_density,
            gravity: self.gravity,
            min_bounce_impulse: self.min_bounce_impulse,
            collision_repulsion: self.collision_repulsion,
            does_tick_time: self.does_tick_time,
            client_collision_repulsion: self.client_collision_repulsion,
            terrain: self.terrain,
            saving: self.saving,
            save_dir: self.save_dir,
            save_interval: self.save_interval,
            command_symbol: self.command_symbol,
            save_entities: self.save_entities,
            client_only_meshing: self.client_only_meshing,
            entity_visible_radius: if self.entity_visible_radius > 0.0 {
                self.entity_visible_radius
            } else {
                24.0 * self.chunk_size as f32
            },
        }
    }
}
