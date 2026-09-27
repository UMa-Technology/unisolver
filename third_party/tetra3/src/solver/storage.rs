//! Pattern-table storage and the mmap-friendly `UNISOLV2` container.
//!
//! LOCAL PATCH (unisolver): upstream tetra3rs keeps the whole database as one
//! postcard blob, which deserializes the open-addressing pattern table (the
//! dominant allocation: slots × 24 B, ~600 MB for a 6.4 M-pattern deep DB)
//! into an owned `Vec`. Measured peak RSS solving with the 2.5–12° deep DB
//! was 1.1 GB — over the 300 MB mobile budget. This module lets the pattern
//! table stay on disk and be demand-paged:
//!
//! ```text
//! 0..8    magic  b"UNISOLV2"
//! 8..12   endian tag u32 LE = 0x1A2B3C4D  (v2 is little-endian only)
//! 12..16  reserved u32 = 0
//! 16..24  head_len u64 LE
//! 24..    postcard(HeadV2)   -- star catalog, vectors, ids, props (small)
//! pad to 8-byte alignment
//! ..end   entries: n × 24 B  -- raw PatternEntry slots, mmap'd zero-copy
//! ```
//!
//! Only the pattern table is mapped; the star-side structures (~50 MB owned
//! for the deep DB) keep their existing types and APIs untouched.

use std::io::{Read, Write};
use std::ops::Deref;
use std::sync::Arc;

use memmap2::Mmap;
use serde::{Deserialize, Serialize};

use super::{DatabaseProperties, PatternEntry, SolverDatabase};
use crate::starcatalog::StarCatalog;

pub const V2_MAGIC: &[u8; 8] = b"UNISOLV2";
const V2_ENDIAN_TAG: u32 = 0x1A2B_3C4D;
const V2_HEADER_LEN: usize = 24;
const ENTRY_SIZE: usize = core::mem::size_of::<PatternEntry>();

// The zero-copy cast below is only sound for the exact layout we serialize:
// #[repr(C)] { [u32; 4], f32, u16, u16 } = 24 bytes, align 4, no implicit
// padding (the trailing u16 is an explicit field), and every bit pattern of
// every field is a valid value. Compile-time guards so a future field edit
// fails the build instead of corrupting databases.
const _: () = assert!(core::mem::size_of::<PatternEntry>() == 24);
const _: () = assert!(core::mem::align_of::<PatternEntry>() == 4);

/// Backing storage for the pattern hash table: an owned `Vec` (generation,
/// legacy postcard files) or a read-only view into a memory-mapped `UNISOLV2`
/// file (demand-paged, near-zero resident until probed).
pub enum PatternStore {
    Owned(Vec<PatternEntry>),
    Mapped {
        map: Arc<Mmap>,
        /// Byte offset of the entries section (validated: in bounds, aligned).
        offset: usize,
        /// Number of `PatternEntry` slots.
        len: usize,
    },
}

impl Deref for PatternStore {
    type Target = [PatternEntry];

    #[inline]
    fn deref(&self) -> &[PatternEntry] {
        match self {
            Self::Owned(v) => v,
            Self::Mapped { map, offset, len } => {
                // Safety: constructor validated offset + len*24 <= map.len()
                // and 8-byte alignment (mmap bases are page-aligned); layout
                // guards above make any byte pattern a valid PatternEntry.
                unsafe {
                    std::slice::from_raw_parts(
                        map.as_ptr().add(*offset) as *const PatternEntry,
                        *len,
                    )
                }
            }
        }
    }
}

// Mutation is the generation path only; databases load mapped for solving.
// DerefMut (rather than a fallible accessor) keeps upstream call sites
// (`get_mut`, tests indexing `entries[i]`) source-compatible.
impl std::ops::DerefMut for PatternStore {
    #[inline]
    fn deref_mut(&mut self) -> &mut [PatternEntry] {
        match self {
            Self::Owned(v) => v,
            Self::Mapped { .. } => {
                panic!("pattern table is memory-mapped (read-only); generation requires Owned")
            }
        }
    }
}

impl PatternStore {
    /// Raw little-endian bytes of the table (for the v2 writer).
    fn as_bytes(&self) -> &[u8] {
        let slice: &[PatternEntry] = self;
        // Safety: 24-byte POD with no padding (guards above); explicit _pad
        // field means every byte is initialized.
        unsafe {
            std::slice::from_raw_parts(slice.as_ptr() as *const u8, std::mem::size_of_val(slice))
        }
    }
}

impl Clone for PatternStore {
    fn clone(&self) -> Self {
        match self {
            Self::Owned(v) => Self::Owned(v.clone()),
            Self::Mapped { map, offset, len } => Self::Mapped {
                map: Arc::clone(map),
                offset: *offset,
                len: *len,
            },
        }
    }
}

impl std::fmt::Debug for PatternStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Owned(v) => write!(f, "PatternStore::Owned({} slots)", v.len()),
            Self::Mapped { len, .. } => write!(f, "PatternStore::Mapped({len} slots)"),
        }
    }
}

// Wire-compatible with the legacy `Vec<PatternEntry>` field: postcard encodes
// a Vec and a slice identically (varint length + elements), so v1 files keep
// round-tripping through the derived SolverDatabase serde.
impl Serialize for PatternStore {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let slice: &[PatternEntry] = self;
        slice.serialize(s)
    }
}

impl<'de> Deserialize<'de> for PatternStore {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Vec::<PatternEntry>::deserialize(d).map(Self::Owned)
    }
}

/// Everything except the pattern table; small enough to deserialize owned.
#[derive(Serialize, Deserialize)]
struct HeadV2 {
    n_entries: u64,
    star_catalog: StarCatalog,
    star_vectors: Vec<[f32; 3]>,
    star_catalog_ids: Vec<i64>,
    props: DatabaseProperties,
}

fn entries_offset(head_len: usize) -> usize {
    (V2_HEADER_LEN + head_len).next_multiple_of(8)
}

/// Write `db` as a `UNISOLV2` file.
pub fn write_v2(db: &SolverDatabase, path: &str) -> crate::Result<()> {
    let head = HeadV2 {
        n_entries: db.pattern_catalog.entries.len() as u64,
        star_catalog: db.star_catalog.clone(),
        star_vectors: db.star_vectors.clone(),
        star_catalog_ids: db.star_catalog_ids.clone(),
        props: db.props.clone(),
    };
    let head_bytes = postcard::to_allocvec(&head)?;

    let mut f = std::io::BufWriter::new(std::fs::File::create(path)?);
    f.write_all(V2_MAGIC)?;
    f.write_all(&V2_ENDIAN_TAG.to_le_bytes())?;
    f.write_all(&0u32.to_le_bytes())?;
    f.write_all(&(head_bytes.len() as u64).to_le_bytes())?;
    f.write_all(&head_bytes)?;
    let pad = entries_offset(head_bytes.len()) - (V2_HEADER_LEN + head_bytes.len());
    f.write_all(&[0u8; 8][..pad])?;
    f.write_all(db.pattern_catalog.entries.as_bytes())?;
    f.flush()?;
    Ok(())
}

/// Sniff whether `path` starts with the `UNISOLV2` magic.
pub fn is_v2_file(path: &str) -> bool {
    let mut magic = [0u8; 8];
    matches!(
        std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut magic)),
        Ok(())
    ) && &magic == V2_MAGIC
}

/// Open a `UNISOLV2` file, memory-mapping the pattern table.
///
/// Head invariants are checked via [`SolverDatabase::validate_head`]; the
/// per-entry star-index bound that `validate` sweeps the whole table for is
/// instead enforced at probe time in the solver (sweeping here would touch
/// every page and defeat demand paging).
pub fn read_v2(path: &str) -> crate::Result<SolverDatabase> {
    use crate::Error::InvalidInput;

    if cfg!(target_endian = "big") {
        return Err(InvalidInput(
            "UNISOLV2 databases are little-endian; big-endian hosts are unsupported".into(),
        ));
    }

    let file = std::fs::File::open(path)?;
    // Safety: read-only map of a locally managed file; concurrent truncation
    // would fault, which is acceptable for app-bundled/downloaded data.
    let map = Arc::new(unsafe { Mmap::map(&file)? });
    if map.len() < V2_HEADER_LEN {
        return Err(InvalidInput("UNISOLV2: file shorter than header".into()));
    }
    if &map[0..8] != V2_MAGIC {
        return Err(InvalidInput("UNISOLV2: bad magic".into()));
    }
    if u32::from_le_bytes(map[8..12].try_into().unwrap()) != V2_ENDIAN_TAG {
        return Err(InvalidInput("UNISOLV2: bad endian tag".into()));
    }
    let head_len = u64::from_le_bytes(map[16..24].try_into().unwrap()) as usize;
    let head_end = V2_HEADER_LEN
        .checked_add(head_len)
        .filter(|&e| e <= map.len())
        .ok_or_else(|| InvalidInput("UNISOLV2: head extends past file".into()))?;
    let head: HeadV2 = postcard::from_bytes(&map[V2_HEADER_LEN..head_end])?;

    let offset = entries_offset(head_len);
    let n = head.n_entries as usize;
    let need = offset
        .checked_add(
            n.checked_mul(ENTRY_SIZE)
                .ok_or_else(|| InvalidInput("UNISOLV2: entry count overflows".into()))?,
        )
        .ok_or_else(|| InvalidInput("UNISOLV2: entries extend past file".into()))?;
    if need > map.len() {
        return Err(InvalidInput(format!(
            "UNISOLV2: file truncated ({} bytes, entries need {need})",
            map.len()
        )));
    }

    let db = SolverDatabase {
        star_catalog: head.star_catalog,
        star_vectors: head.star_vectors,
        star_catalog_ids: head.star_catalog_ids,
        pattern_catalog: super::PatternCatalog {
            entries: PatternStore::Mapped {
                map,
                offset,
                len: n,
            },
        },
        props: head.props,
    };
    db.validate_head()?;
    Ok(db)
}
