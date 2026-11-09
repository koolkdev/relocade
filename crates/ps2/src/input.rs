//! Retain each input and its synchronization wait across firmware selections.

use futures_util::{
    stream::{self, LocalBoxStream},
    Stream, StreamExt,
};
use relocade_frontend::input::InputEvent;
use relocade_machine::Clock;

pub(super) fn synchronized<T: 'static>(
    clock: Clock,
    events: impl Stream<Item = InputEvent<T>> + Unpin + 'static,
) -> LocalBoxStream<'static, T> {
    stream::unfold(events, move |mut events| {
        let clock = clock.clone();
        async move {
            match events.next().await {
                Some(input) => {
                    clock.sync_to(input.host_time).await;
                    Some((input.event, events))
                }
                None => {
                    // Closure releases held input, so it obeys the same ordering
                    // rule using the time the consumer observes disconnection.
                    clock.sync().await;
                    None
                }
            }
        }
    })
    .boxed_local()
}
