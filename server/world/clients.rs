use hashbrown::HashMap;

use specs::Entity;

use crate::{server::WsSender, ConnectionPrincipal};

/// A client of the server.
#[derive(Clone)]
pub struct Client {
    /// The client's ID on the voxelize server.
    pub id: String,

    /// The username of the client.
    pub username: String,

    /// The entity that represents this client in the ECS world.
    pub entity: Entity,

    /// WebSocket sender to the client.
    pub sender: WsSender,

    /// Authenticated owner. `None` is reserved for legacy sessions.
    pub principal: Option<ConnectionPrincipal>,

    /// Detached clients keep their entity but cannot receive or submit requests.
    pub attached: bool,

    /// Admission lease that created this client entity.
    pub(crate) join_attempt_id: String,
}

pub type Clients = HashMap<String, Client>;
