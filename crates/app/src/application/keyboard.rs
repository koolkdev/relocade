use relocade_frontend::keyboard::KeyboardEvent;
use winit::event::WindowEvent;

pub(super) fn translate(event: &WindowEvent) -> Option<KeyboardEvent> {
    Some(match event {
        WindowEvent::KeyboardInput {
            event,
            is_synthetic: false,
            ..
        } => KeyboardEvent::Key {
            physical_key: event.physical_key,
            logical_key: event.logical_key.clone(),
            location: event.location,
            text: event.text.as_ref().map(ToString::to_string),
            state: event.state,
            repeat: event.repeat,
        },
        WindowEvent::ModifiersChanged(modifiers) => KeyboardEvent::Modifiers(modifiers.state()),
        WindowEvent::Focused(false) => KeyboardEvent::FocusLost,
        _ => return None,
    })
}
