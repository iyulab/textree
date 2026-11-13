//! Writing text into documents: appending, editing one section, and several written-text
//! operations planned as one change.

use std::collections::BTreeMap;

use textree::{plan, plan_many, recheck, MemoryStore, Op, PlanError, Policy, Snapshot};

fn snapshot(list: &[(&str, &str)]) -> Snapshot {
    let mut s = MemoryStore::new();
    for (p, c) in list {
        s = s.with_file(p, c.as_bytes());
    }
    Snapshot::read(&s)
}

fn written(cs: &textree::ChangeSet) -> BTreeMap<String, String> {
    cs.creates
        .iter()
        .chain(cs.writes.iter())
        .map(|w| (w.path.clone(), w.content.clone()))
        .collect()
}

const WATERING: &str = "# Watering\n\nIn the morning.\n\n## Schedule\n\n- May: every two days.\n\n## Tools\n\nA can.\n";

#[test]
fn append_adds_after_a_blank_line_and_keeps_everything_before() {
    let snap = snapshot(&[("garden/watering.md", WATERING)]);
    let op = Op::Append {
        path: "garden/watering.md".into(),
        content: "## Drip irrigation\n\nHalf the water.".into(),
    };
    let p = plan(&snap, &op, &Policy::default()).unwrap();
    let text = &written(&p.changes)["garden/watering.md"];
    assert!(text.starts_with(WATERING));
    assert_eq!(
        &text[WATERING.len()..],
        "\n## Drip irrigation\n\nHalf the water.\n"
    );
    assert_eq!(p.changes.ops, vec![op]);
}

#[test]
fn edit_section_replaces_only_the_body_under_the_heading() {
    let snap = snapshot(&[("garden/watering.md", WATERING)]);
    let op = Op::EditSection {
        path: "garden/watering.md".into(),
        heading: "Schedule".into(),
        content: "- May: every two days.\n- June: every day.".into(),
    };
    let p = plan(&snap, &op, &Policy::default()).unwrap();
    let text = &written(&p.changes)["garden/watering.md"];
    assert_eq!(
        text,
        "# Watering\n\nIn the morning.\n\n## Schedule\n\n- May: every two days.\n- June: every day.\n\n## Tools\n\nA can.\n"
    );
}

#[test]
fn edit_section_of_the_last_section_and_a_missing_or_doubled_heading() {
    let snap = snapshot(&[
        ("a.md", "# A\n\nx\n\n## Last\n\nold\n"),
        ("b.md", "# B\n\n## Same\n\n1\n\n## Same\n\n2\n"),
    ]);
    let p = plan(
        &snap,
        &Op::EditSection {
            path: "a.md".into(),
            heading: "last".into(),
            content: "new".into(),
        },
        &Policy::default(),
    )
    .unwrap();
    assert_eq!(written(&p.changes)["a.md"], "# A\n\nx\n\n## Last\n\nnew\n");
    for (path, heading) in [("a.md", "Nope"), ("b.md", "Same")] {
        let r = plan(
            &snap,
            &Op::EditSection {
                path: path.into(),
                heading: heading.into(),
                content: "z".into(),
            },
            &Policy::default(),
        );
        assert!(
            matches!(r, Err(PlanError::Usage(_))),
            "{path} {heading}: {r:?}"
        );
    }
}

#[test]
fn edit_section_that_drops_a_heading_another_document_links_to_is_refused() {
    let snap = snapshot(&[
        ("a.md", "# A\n\n## Part\n\n### Detail\n\nd\n"),
        ("b.md", "See [d](a.md#detail).\n"),
    ]);
    let r = plan(
        &snap,
        &Op::EditSection {
            path: "a.md".into(),
            heading: "Part".into(),
            content: "gone".into(),
        },
        &Policy::default(),
    );
    assert!(matches!(r, Err(PlanError::Refused(_))), "{r:?}");
}

#[test]
fn several_written_text_operations_are_one_plan_and_recheck_to_the_same_plan() {
    let snap = snapshot(&[
        ("garden/watering.md", WATERING),
        ("reading/books.md", "# Books\n"),
    ]);
    let ops = vec![
        Op::Append {
            path: "garden/watering.md".into(),
            content: "## Drip\n\nHalf.".into(),
        },
        Op::Add {
            path: "reading/habits.md".into(),
            content: "# Habits\n\nSee [books](books.md).".into(),
        },
        Op::Add {
            path: "kitchen/kitchen.md".into(),
            content: "# Kitchen\n".into(),
        },
    ];
    let p = plan_many(&snap, &ops, &Policy::default()).unwrap();
    assert_eq!(p.changes.ops, ops);
    let w = written(&p.changes);
    assert!(w["garden/watering.md"].ends_with("## Drip\n\nHalf.\n"));
    assert!(w.contains_key("reading/habits.md") && w.contains_key("kitchen/kitchen.md"));
    assert_eq!(p.changes.creates.len(), 2);
    assert!(p.changes.new_broken.is_empty());
    let again = recheck(&snap, &p.changes).unwrap();
    assert_eq!(again.changes, p.changes);
}

#[test]
fn several_operations_refuse_structure_and_a_document_twice() {
    let snap = snapshot(&[("a.md", "# A\n"), ("b.md", "# B\n")]);
    let twice = vec![
        Op::Append {
            path: "a.md".into(),
            content: "x".into(),
        },
        Op::Append {
            path: "a.md".into(),
            content: "y".into(),
        },
    ];
    assert!(matches!(
        plan_many(&snap, &twice, &Policy::default()),
        Err(PlanError::Usage(_))
    ));
    let mixed = vec![
        Op::Append {
            path: "a.md".into(),
            content: "x".into(),
        },
        Op::Delete {
            path: "b.md".into(),
        },
    ];
    assert!(matches!(
        plan_many(&snap, &mixed, &Policy::default()),
        Err(PlanError::Usage(_))
    ));
}
