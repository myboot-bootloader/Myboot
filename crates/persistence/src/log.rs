//! Tier-2 boot log: an append-only, line-delimited history on the ESP
//! (report §3.9, §5). Growing data lives in a file, NEVER in NVRAM. Each record
//! is one self-describing text line so the log survives partial writes and can be
//! exported for diagnostics.
extern crate alloc;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use graph::{BootResult, EntryId};
use ports::{FileStore, IoResult};

pub const LOG_PATH: &str = "\\EFI\\MyBoot\\log\\boot.log";

/// One boot-history record. `seq` is a monotonically increasing counter kept by
/// the caller; `phase` is a short stage tag; `result` is the outcome if known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRecord {
    pub seq: u64,
    pub entry: EntryId,
    pub phase: &'static str,
    pub result: Option<BootResult>,
}

impl LogRecord {
    /// Render one line: `seq<TAB>entry<TAB>phase<TAB>result\n`. Stable, greppable,
    /// and free of user-controlled newlines (the id is a normalised slug).
    pub fn render(&self) -> String {
        let r = match self.result {
            Some(BootResult::Confirmed) => "confirmed",
            Some(BootResult::Failed) => "failed",
            Some(BootResult::Unknown) | None => "-",
        };
        format!(
            "{}\t{}\t{}\t{}\n",
            self.seq,
            self.entry.as_str(),
            self.phase,
            r
        )
    }
}

/// Append a record to the log via the `FileStore` port (creates the file if
/// absent). One responsibility: turn a record into bytes and hand it to the port.
pub fn append_record<F: FileStore>(fs: &mut F, rec: &LogRecord) -> IoResult<()> {
    fs.append(LOG_PATH, rec.render().as_bytes())
}

/// Parse the log back into records (for the shell's `log` command and export).
/// Total: malformed lines are skipped, never panicked.
pub fn parse(data: &[u8]) -> Vec<(u64, String, String, String)> {
    let text = match core::str::from_utf8(data) {
        Ok(t) => t,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for line in text.lines() {
        let mut it = line.splitn(4, '\t');
        // A line without all four fields is malformed and skipped.
        if let (Some(seq), Some(entry), Some(phase), Some(result)) =
            (it.next(), it.next(), it.next(), it.next())
        {
            if let Ok(n) = seq.parse::<u64>() {
                out.push((
                    n,
                    String::from(entry),
                    String::from(phase),
                    String::from(result),
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph::{BootResult, EntryId, EntryRole, OsKind};
    use ports::mock::MockFileStore;

    #[test]
    fn append_then_parse_roundtrips_records() {
        let mut fs = MockFileStore::new();
        let id = EntryId::derive(OsKind::NixOs, &EntryRole::Generation(128), None);
        append_record(
            &mut fs,
            &LogRecord {
                seq: 1,
                entry: id.clone(),
                phase: "staged",
                result: None,
            },
        )
        .unwrap();
        append_record(
            &mut fs,
            &LogRecord {
                seq: 2,
                entry: id.clone(),
                phase: "confirmed",
                result: Some(BootResult::Confirmed),
            },
        )
        .unwrap();
        let raw = fs.read(LOG_PATH).unwrap();
        let recs = parse(&raw);
        assert_eq!(recs.len(), 2);
        assert_eq!(recs[0].0, 1);
        assert_eq!(recs[1].3, "confirmed");
    }

    #[test]
    fn parse_skips_malformed_lines() {
        let recs = parse(b"garbage line without tabs\n5\tnixos:gen1\tstaged\t-\n");
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].0, 5);
    }
}
