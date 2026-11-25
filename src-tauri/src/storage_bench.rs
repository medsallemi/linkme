use super::*;
use std::{hint::black_box, time::Instant};

fn summary(size: usize, scenario: &str, samples: &mut [f64]) {
    samples.sort_by(f64::total_cmp);
    let percentile = |q: f64| samples[(samples.len() as f64 * q).ceil() as usize - 1];
    eprintln!(
        "BENCH links={size} {scenario} n={} p50={:.3}ms p95={:.3}ms max={:.3}ms",
        samples.len(),
        percentile(0.50),
        percentile(0.95),
        samples.last().unwrap()
    );
}

#[test]
#[ignore = "isolated performance benchmark; run alone in release mode"]
fn library_performance_baseline() {
    for size in [1_000_i64, 10_000] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("synthetic.sqlite3");
        let mut db = Database::open(&path).unwrap();
        let tx = db.connection.transaction().unwrap();
        for category in 2..=40 {
            tx.execute(
                "INSERT INTO categories(id,name) VALUES(?1,?2)",
                params![category, format!("Collection {category}")],
            )
            .unwrap();
        }
        for group in 1..=40 {
            tx.execute(
                "INSERT INTO link_groups(id,category_id,name) VALUES(?1,?1,?2)",
                params![group, format!("Reading group {group}")],
            )
            .unwrap();
        }
        for n in 0..size {
            let category = n % 40 + 1;
            tx.execute(
                "INSERT INTO links(url,title,subtitle,caption,category_id,group_id,sort_id)
                        VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![
                    format!("https://example.invalid/article/{n}?sort=latest#section"),
                    format!("Reference document {n}"),
                    if n % 7 == 0 {
                        "A practical guide to local desktop software"
                    } else {
                        "Notes from the previous review"
                    },
                    format!(
                        "{} token-{n}",
                        "Representative caption notes. ".repeat((n % 31) as usize)
                    ),
                    category,
                    (n % 3 != 0).then_some(category),
                    (n == 0).then_some(size),
                ],
            )
            .unwrap();
        }
        // Swap two browse slots so the benchmark exercises custom ordering.
        tx.execute("UPDATE links SET sort_id=1 WHERE id=?1", [size])
            .unwrap();
        tx.commit().unwrap();
        drop(db);
        let mut db = Database::open(&path).unwrap();
        let first = db.list("", None).unwrap();
        assert_eq!(first.links[0].id, 1);
        let cursor = first.next.unwrap();
        for (label, query, before) in [
            ("browse_first", "", None),
            ("browse_next", "", Some(cursor)),
            ("search_broad", "Reference", None),
            ("search_exact", "Reference document 42", None),
            ("search_group", "Reading group 17", None),
            ("search_caption", "token-23", None),
            ("search_miss", "%_", None),
        ] {
            for _ in 0..6 {
                black_box(db.list(query, before).unwrap());
            }
            let mut samples: Vec<_> = (0..60)
                .map(|_| {
                    let start = Instant::now();
                    let result = db.list(query, before).unwrap();
                    let elapsed = start.elapsed().as_secs_f64() * 1000.0;
                    black_box(result);
                    elapsed
                })
                .collect();
            summary(size as usize, label, &mut samples);
        }
        let mut saves = Vec::new();
        for n in 0..66 {
            let input = draft(&format!("https://example.invalid/new/{n}"));
            let start = Instant::now();
            assert!(db.save(&input).unwrap().created);
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            if n >= 6 {
                saves.push(elapsed);
            }
        }
        summary(size as usize, "save_commit", &mut saves);
    }
}
