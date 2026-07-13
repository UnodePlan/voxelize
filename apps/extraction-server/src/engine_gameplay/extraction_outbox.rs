use specs::{Entities, Join, ReadStorage, WriteStorage};

use super::{
    authority::GameplayAuthority,
    components::{EliminationComp, ExtractionComp, MatchPlayerComp},
};
pub(super) fn flush_extraction_notices<'a>(
    entities: &Entities<'a>,
    authority: &GameplayAuthority,
    players: &ReadStorage<'a, MatchPlayerComp>,
    extractions: &mut WriteStorage<'a, ExtractionComp>,
) {
    for (_, _player, extraction) in (entities, players, extractions).join() {
        let Some(record) = extraction.record_mut() else {
            continue;
        };
        if record.notice_sent {
            continue;
        }
        if authority.report_extraction(record.qualification.clone()) {
            record.notice_sent = true;
        }
    }
}

pub(super) fn terminal_outboxes_are_flushed<'a>(
    entities: &Entities<'a>,
    eliminations: &WriteStorage<'a, EliminationComp>,
    extractions: &WriteStorage<'a, ExtractionComp>,
) -> bool {
    (entities, eliminations)
        .join()
        .all(|(_, elimination)| elimination.record().is_none_or(|record| record.notice_sent))
        && (entities, extractions)
            .join()
            .all(|(_, extraction)| extraction.record().is_none_or(|record| record.notice_sent))
}
