use super::{
    machine::{both, Exit, Image, Step},
    step::TestModule,
};

pub(crate) fn check_conditions(
    step: &TestModule,
    name: &str,
    prefix: &[u8],
    image: &mut Image,
    prefix_updates: &[(usize, u32)],
    conditions: u16,
) {
    let mut code = prefix.to_vec();
    for condition in 0..16 {
        // ModRM.reg is ignored by SETcc. Every possible value appears here.
        code.extend_from_slice(&[
            0x0f,
            0x90 + condition,
            0x47 | ((condition & 7) << 3),
            condition,
        ]);
    }
    image.data(0x3000, &code);
    image.register(52, 0x6000);
    image.map(6, 0xa000, true);
    image.data(0x9fff, &[0xa5; 18]);
    let results = (0..16)
        .map(|condition| [((conditions >> condition) & 1) as u8])
        .collect::<Vec<_>>();
    let mut updates = Vec::new();
    if !prefix.is_empty() {
        let mut changes = prefix_updates.to_vec();
        changes.extend_from_slice(&[(56, 0x1000 + prefix.len() as u32), (144, 0)]);
        updates.push(changes);
    }
    for condition in 0..16 {
        updates.push(vec![
            (56, 0x1000 + prefix.len() as u32 + 4 * (condition + 1)),
            (144, condition + u32::from(!prefix.is_empty())),
        ]);
    }
    let writes = results
        .iter()
        .enumerate()
        .map(|(condition, result)| [(0xa000 + condition as u32, result.as_slice())])
        .collect::<Vec<_>>();
    let mut steps = Vec::new();
    if !prefix.is_empty() {
        steps.push(Step {
            cpu: &updates[0],
            ram: &[],
            exit: Exit::Dispatch(0x1000 + prefix.len() as u32),
        });
    }
    for condition in 0..16 {
        steps.push(Step {
            cpu: &updates[condition + usize::from(!prefix.is_empty())],
            ram: &writes[condition],
            exit: Exit::Dispatch(0x1000 + prefix.len() as u32 + 4 * (condition as u32 + 1)),
        });
    }
    both(step, name, &code, steps.len() as u32, image, &steps);
}
