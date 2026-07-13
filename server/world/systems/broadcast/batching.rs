use hashbrown::HashMap;

use crate::{ClientFilter, Message, MessageType};

fn filter_key(filter: &ClientFilter) -> String {
    match filter {
        ClientFilter::All => "all".to_string(),
        ClientFilter::Direct(id) => format!("direct:{id}"),
        ClientFilter::Include(ids) => {
            let mut sorted = ids.clone();
            sorted.sort();
            format!("include:{}", sorted.join(","))
        }
        ClientFilter::Exclude(ids) => {
            let mut sorted = ids.clone();
            sorted.sort();
            format!("exclude:{}", sorted.join(","))
        }
    }
}

fn can_batch(msg_type: i32) -> bool {
    matches!(
        MessageType::try_from(msg_type),
        Ok(MessageType::Peer)
            | Ok(MessageType::Entity)
            | Ok(MessageType::Update)
            | Ok(MessageType::Event)
    )
}

fn merge_messages(base: &mut Message, other: Message) {
    base.peers.extend(other.peers);
    base.entities.extend(other.entities);
    base.updates.extend(other.updates);
    base.events.extend(other.events);
}

pub(super) fn batch_messages(
    messages: Vec<(Message, ClientFilter)>,
) -> Vec<(Message, ClientFilter)> {
    let mut result: Vec<(Message, ClientFilter)> = Vec::with_capacity(messages.len());
    let mut batch_indices: HashMap<(i32, String), usize> = HashMap::new();

    for (message, filter) in messages {
        if !can_batch(message.r#type) {
            result.push((message, filter));
            continue;
        }

        let key = (message.r#type, filter_key(&filter));
        if let Some(index) = batch_indices.get(&key).copied() {
            merge_messages(&mut result[index].0, message);
            continue;
        }

        batch_indices.insert(key, result.len());
        result.push((message, filter));
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PeerProtocol;

    fn peer(id: &str) -> Message {
        Message::new(&MessageType::Peer)
            .peers(&[PeerProtocol {
                id: id.to_owned(),
                username: id.to_owned(),
                metadata: "{}".to_owned(),
            }])
            .build()
    }

    #[test]
    fn batching_preserves_each_groups_first_position_and_join_before_peer() {
        let direct = || ClientFilter::Direct("viewer".to_owned());
        let messages = vec![
            (
                Message::new(&MessageType::Join).text("near").build(),
                direct(),
            ),
            (peer("near"), direct()),
            (
                Message::new(&MessageType::Leave).text("far").build(),
                direct(),
            ),
            (Message::new(&MessageType::Entity).build(), direct()),
            (peer("self"), direct()),
            (Message::new(&MessageType::Event).build(), ClientFilter::All),
            (Message::new(&MessageType::Entity).build(), direct()),
        ];

        let batched = batch_messages(messages);
        let types = batched
            .iter()
            .map(|(message, _)| MessageType::try_from(message.r#type).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(
            types,
            vec![
                MessageType::Join,
                MessageType::Peer,
                MessageType::Leave,
                MessageType::Entity,
                MessageType::Event,
            ]
        );
        assert_eq!(batched[1].0.peers.len(), 2);
    }
}
