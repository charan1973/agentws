use agentws::manifest::{self, RepoRequest, Workspace};
use chrono::Utc;
use std::sync::{Arc, Barrier};
use std::time::Duration;

#[test]
fn concurrent_mutations_preserve_every_request_and_event() {
    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    std::env::set_var("HOME", &home);
    std::env::remove_var("AGENTWS_WORKSPACE");

    let story = "concurrent-manifest";
    let root = home.join(".agentws").join(story);
    manifest::save(&Workspace {
        story: story.into(),
        root,
        created: Utc::now(),
        repos: vec![],
        requests: vec![],
        archived: false,
        skills: vec![],
        agents_md: vec![],
        setup: Default::default(),
    })
    .unwrap();

    const WRITERS: usize = 8;
    let barrier = Arc::new(Barrier::new(WRITERS));
    let handles: Vec<_> = (0..WRITERS)
        .map(|index| {
            let barrier = Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                manifest::mutate(story, |workspace| {
                    // Hold the transaction briefly so competitors exercise
                    // SQLite's busy wait instead of serializing by chance.
                    std::thread::sleep(Duration::from_millis(20));
                    workspace.requests.push(RepoRequest {
                        id: format!("request-{index}"),
                        repo: format!("repo-{index}"),
                        reason: Some("concurrency test".into()),
                        status: "pending".into(),
                        by: "agent".into(),
                        created: Utc::now(),
                        resolved: None,
                    });
                    Ok(())
                })
                .unwrap();
            })
        })
        .collect();

    for handle in handles {
        handle.join().unwrap();
    }

    let workspace = manifest::load(story).unwrap();
    assert_eq!(workspace.requests.len(), WRITERS);
    let history = manifest::request_history(story).unwrap();
    assert_eq!(history.len(), WRITERS);
    assert!(history.iter().all(|event| event.status == "pending"));
}
