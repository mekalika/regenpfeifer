mod cache;
mod emphasize;
mod generator;
mod matcher;
mod patterns;
mod split;
mod stroke;

use emphasize::Emphasizer;
use generator::Generator;
use matcher::Matcher;
use rayon::prelude::*;
use split::WordSplitter;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    // args: [prog] [wordlist.csv] [out.json] [assets_dir] [limit]
    let wordlist = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "/tmp/wortformliste.csv".to_string());
    let out_path = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "/tmp/regen-rs-out.json".to_string());
    let assets_dir = args
        .get(3)
        .cloned()
        .unwrap_or_else(|| "/tmp/fork-gen/regenpfeifer/assets".to_string());
    let limit: usize = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(0);

    // Validation mode: REGEN_DUMP=<words_file> -> print word\t[outlines json] using
    // the SAME trie/word-list, then exit. Lets us diff against the Python reference.
    let dump_words = std::env::var("REGEN_DUMP").ok();

    let assets = PathBuf::from(&assets_dir);
    let patterns_dir = assets.join("patterns");

    let t_total = Instant::now();

    // --- load patterns ---
    let vowel = patterns::load(&patterns_dir.join("vowel_patterns.json"));
    let left = patterns::load(&patterns_dir.join("left_patterns.json"));
    let right = patterns::load(&patterns_dir.join("right_patterns.json"));

    // --- load custom translations (seeded first) ---
    let custom_path = assets.join("dictionaries/custom_translations.json");
    let custom_text = std::fs::read_to_string(&custom_path).unwrap_or_default();
    let custom: serde_json::Value =
        serde_json::from_str(&custom_text).unwrap_or(serde_json::Value::Null);
    let mut custom_pairs: Vec<(String, String)> = Vec::new();
    let mut custom_translated_words: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    if let Some(obj) = custom.as_object() {
        for (k, v) in obj {
            let val = v.as_str().unwrap_or("").to_string();
            custom_translated_words.insert(val.clone());
            custom_pairs.push((k.clone(), val));
        }
    }

    // --- read word list ---
    let t_read = Instant::now();
    let raw = std::fs::read_to_string(&wordlist).expect("cannot read word list");
    let mut rows: Vec<(String, String)> = Vec::new();
    for line in raw.lines() {
        // split on first comma into word,type-rest (Python: line.split(','))
        let mut it = line.splitn(2, ',');
        if let (Some(w), Some(rest)) = (it.next(), it.next()) {
            // word_type = rest.split(' ')[0].replace('\n','')
            let wt = rest.split(' ').next().unwrap_or("").trim_end().to_string();
            rows.push((w.to_string(), wt));
        }
    }
    // words used by the trie AND generated: len(chars) > 3
    let gen_rows: Vec<(String, String)> = rows
        .into_iter()
        .filter(|(w, _)| w.chars().count() > 3)
        .collect();
    let trie_words: Vec<String> = gen_rows.iter().map(|(w, _)| w.clone()).collect();
    eprintln!(
        "read {} words (len>3) in {:.1}s",
        gen_rows.len(),
        t_read.elapsed().as_secs_f64()
    );

    // --- build generator (shared, read-only) ---
    let t_build = Instant::now();
    let splitter = WordSplitter::new(&trie_words);
    let generator = Generator::new(splitter, Emphasizer::new(), Matcher::new(vowel, left, right));
    eprintln!("trie/generator built in {:.1}s", t_build.elapsed().as_secs_f64());

    // --- validation dump mode ---
    if let Some(dump_path) = dump_words {
        let dump_raw = std::fs::read_to_string(&dump_path).expect("cannot read dump words");
        let words: Vec<String> = dump_raw.lines().map(|l| l.to_string()).collect();
        // need word_type per word; look it up from gen_rows
        let type_map: std::collections::HashMap<&str, &str> =
            gen_rows.iter().map(|(w, t)| (w.as_str(), t.as_str())).collect();
        for w in &words {
            let wt = type_map.get(w.as_str()).copied().unwrap_or("other");
            let outs = generator.generate(w, wt);
            println!("{}\t{}", w, serde_json::to_string(&outs).unwrap());
        }
        return;
    }

    // --- generate in parallel ---
    let work: Vec<(String, String)> = if limit > 0 {
        gen_rows.iter().take(limit).cloned().collect()
    } else {
        gen_rows.clone()
    };

    let t_gen = Instant::now();
    let results: Vec<(String, Vec<String>)> = work
        .par_iter()
        .map(|(word, wt)| {
            if custom_translated_words.contains(word) {
                (word.clone(), Vec::new())
            } else {
                (word.clone(), generator.generate(word, wt))
            }
        })
        .collect();
    eprintln!(
        "generated {} words in {:.1}s",
        work.len(),
        t_gen.elapsed().as_secs_f64()
    );
    {
        use std::sync::atomic::Ordering::Relaxed;
        let h = cache::HITS.load(Relaxed);
        let m = cache::MISSES.load(Relaxed);
        let mn = cache::MATCH_NANOS.load(Relaxed) as f64 / 1e9;
        eprintln!(
            "  cache: {} hits, {} misses ({:.1}% hit) | matcher sum-across-threads {:.1}s",
            h, m, 100.0 * h as f64 / (h + m).max(1) as f64, mn
        );
    }

    // --- assemble dictionary: custom first, then first-wins dedup in word order ---
    let mut dict: BTreeMap<String, String> = BTreeMap::new();
    // seed custom first
    for (k, v) in &custom_pairs {
        dict.entry(k.clone()).or_insert_with(|| v.clone());
    }
    // first-wins: only insert if outline not already present
    for (word, outlines) in &results {
        if custom_translated_words.contains(word) {
            continue;
        }
        for o in outlines {
            dict.entry(o.clone()).or_insert_with(|| word.clone());
        }
    }

    // --- write JSON (sorted, like Python json.dump(sorted(...), indent=0)) ---
    write_dict(&out_path, &dict);

    let n_words = work.len();
    let m_entries = dict.len();
    println!(
        "{} words -> {} entries in {:.1}s",
        n_words,
        m_entries,
        t_total.elapsed().as_secs_f64()
    );
}

/// Write the dict as JSON with indent=0 and ensure_ascii=False, matching the
/// Python output format closely (one key per line). Keys are already sorted via
/// BTreeMap.
fn write_dict(path: &str, dict: &BTreeMap<String, String>) {
    use std::io::Write;
    let file = std::fs::File::create(Path::new(path)).expect("cannot create output");
    let mut w = std::io::BufWriter::new(file);
    // Match Python json.dump(..., indent=0): each item on its own line, no spaces.
    write!(w, "{{").unwrap();
    let mut first = true;
    for (k, v) in dict {
        if first {
            writeln!(w).unwrap();
            first = false;
        } else {
            writeln!(w, ",").unwrap();
        }
        let ks = serde_json::to_string(k).unwrap();
        let vs = serde_json::to_string(v).unwrap();
        write!(w, "{ks}: {vs}").unwrap();
    }
    if !first {
        writeln!(w).unwrap();
    }
    write!(w, "}}").unwrap();
    w.flush().unwrap();
}
