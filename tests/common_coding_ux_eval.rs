use localagent::eval::tasks::{tasks_for_pack, EvalPack};

#[test]
fn common_coding_ux_pack_exposes_first_vertical_slice() {
    let tasks = tasks_for_pack(EvalPack::CommonCodingUx);
    let ids = tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>();

    assert!(ids.contains(&"U1"));
    assert!(ids.contains(&"U2"));
    assert!(ids.contains(&"U3"));
    assert!(ids.contains(&"U4"));
    assert!(ids.contains(&"U5"));
    assert!(ids.contains(&"U6"));
    assert!(ids.contains(&"U7"));
    assert!(ids.contains(&"U9"));
    assert!(ids.contains(&"U12"));
    assert!(ids.iter().all(|id| id.starts_with('U')));
}

#[test]
fn all_pack_includes_common_coding_ux_tasks() {
    let tasks = tasks_for_pack(EvalPack::All);
    let ids = tasks.iter().map(|t| t.id.as_str()).collect::<Vec<_>>();

    for id in ["U1", "U2", "U3", "U4", "U5", "U6", "U7", "U9", "U12"] {
        assert!(ids.contains(&id));
    }
}
