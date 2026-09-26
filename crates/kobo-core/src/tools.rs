//! Running the toolchain's programs on a ROM.
//!
//! Asar runs in the process through its library ([`crate::asar`]). The
//! others are programs the user supplies, run on a copy of the ROM in a
//! scratch folder of their own; what they change is checked with
//! [`rats::Snapshot`] as Asar's patches are. docs/toolchain.md has what
//! each requires of a ROM.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use sha1::{Digest, Sha1};
use thiserror::Error;

use crate::addr::SnesAddr;
use crate::rats::{self, Damage};
use crate::rom::{Rom, RomError};

#[derive(Debug, Error)]
pub enum ToolError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{tool} failed (exit {status}):\n{output}")]
    Failed {
        tool: &'static str,
        status: String,
        output: String,
    },
    #[error("{tool} damaged {} tagged blocks; the first: {}", damage.len(), damage[0])]
    Damaged {
        tool: &'static str,
        damage: Vec<Damage>,
    },
    #[error(transparent)]
    Rom(#[from] RomError),
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> ToolError + '_ {
    move |source| ToolError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Hashes a file or a folder tree: every file's path relative to `root`
/// and its contents, in sorted order, so the hash changes whenever what a
/// tool could read there does.
pub fn hash_tree(hash: &mut Sha1, root: &Path) -> Result<(), ToolError> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort();
    for relative in files {
        // A file root is itself, with an empty relative path.
        let path = if relative.as_os_str().is_empty() {
            root.to_path_buf()
        } else {
            root.join(&relative)
        };
        hash.update(relative.to_string_lossy().replace('\\', "/").as_bytes());
        hash.update([0]);
        let bytes = fs::read(&path).map_err(io_error(&path))?;
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
    }
    Ok(())
}

fn collect_files(root: &Path, at: &Path, files: &mut Vec<PathBuf>) -> Result<(), ToolError> {
    let meta = fs::metadata(at).map_err(io_error(at))?;
    if meta.is_file() {
        files.push(at.strip_prefix(root).unwrap_or(at).to_path_buf());
        return Ok(());
    }
    for entry in fs::read_dir(at).map_err(io_error(at))? {
        let entry = entry.map_err(io_error(at))?;
        collect_files(root, &entry.path(), files)?;
    }
    Ok(())
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), ToolError> {
    if fs::metadata(from).map_err(io_error(from))?.is_file() {
        fs::copy(from, to).map_err(io_error(from))?;
        return Ok(());
    }
    fs::create_dir_all(to).map_err(io_error(to))?;
    for entry in fs::read_dir(from).map_err(io_error(from))? {
        let entry = entry.map_err(io_error(from))?;
        copy_tree(&entry.path(), &to.join(entry.file_name()))?;
    }
    Ok(())
}

/// A scratch folder, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Result<Self, ToolError> {
        static COUNT: AtomicU32 = AtomicU32::new(0);
        let n = COUNT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("kobo-{name}-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).map_err(io_error(&dir))?;
        Ok(Self(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// The name AddmusicK loads Asar's library by on this platform.
const ASAR_LIBRARY_NAME: &str = if cfg!(windows) {
    "asar.dll"
} else if cfg!(target_os = "macos") {
    "libasar.dylib"
} else {
    "libasar.so"
};

/// A program run in a copy of its own folder, the way the toolchain's
/// programs expect: with the project's files laid over the copy and Asar's
/// library beside it, on `rom.sfc` there.
struct FolderTool<'a> {
    name: &'static str,
    /// The program's file name, without `.exe`.
    program: &'static str,
    args: &'a [&'a str],
    /// Files the copy must not keep, such as an options file that would
    /// replace the arguments.
    remove: &'a [&'a str],
    /// Blocks the tool may rewrite in place, as their interface has it:
    /// damage to them is not damage.
    owns: fn(&Rom) -> Vec<SnesAddr>,
}

impl FolderTool<'_> {
    fn run(&self, rom: &Rom, tool: &Path, overlay: &Path, asar: &Path) -> Result<Rom, ToolError> {
        let scratch = Scratch::new(self.program)?;
        let work = &scratch.0;
        copy_tree(tool, work)?;
        copy_tree(overlay, work)?;
        for file in self.remove {
            let _ = fs::remove_file(work.join(file));
        }
        let library = work.join(ASAR_LIBRARY_NAME);
        fs::copy(asar, &library).map_err(io_error(asar))?;
        let rom_path = work.join("rom.sfc");
        rom.save(&rom_path)?;
        let program = work.join(if cfg!(windows) {
            format!("{}.exe", self.program)
        } else {
            self.program.to_owned()
        });
        let output = Command::new(&program)
            .args(self.args)
            .current_dir(work)
            .stdin(Stdio::null())
            .env("LD_LIBRARY_PATH", work)
            // .NET programs need no ICU this way, which some systems lack.
            .env("DOTNET_SYSTEM_GLOBALIZATION_INVARIANT", "1")
            .output()
            .map_err(io_error(&program))?;
        let text = String::from_utf8_lossy(&output.stdout).into_owned()
            + &String::from_utf8_lossy(&output.stderr);
        if !output.status.success() {
            return Err(ToolError::Failed {
                tool: self.name,
                status: output.status.to_string(),
                output: text,
            });
        }
        let after = Rom::from_bytes(fs::read(&rom_path).map_err(io_error(&rom_path))?)?;
        // By file offset: a pointer may name a block through a mirror.
        let owned: Vec<_> = (self.owns)(rom)
            .into_iter()
            .filter_map(|at| rom.pc(at).ok())
            .collect();
        let damage: Vec<_> = rats::Snapshot::take(rom)
            .check(&after, &[])
            .into_iter()
            .filter(|d| rom.pc(d.block.start).is_ok_and(|pc| !owned.contains(&pc)))
            .collect();
        if !damage.is_empty() {
            return Err(ToolError::Damaged {
                tool: self.name,
                damage,
            });
        }
        Ok(after)
    }
}

/// Runs GPS on a copy of `rom`, in a copy of its folder `tool` with the
/// project's GPS folder (`list.txt`, `blocks/`, `routines/`) laid over it.
/// GPS patches the acts-like chain in bank `$06`, which Kobo's install has
/// in the shape GPS expects (docs/toolchain.md).
pub fn gps(rom: &Rom, tool: &Path, files: &Path, asar: &Path) -> Result<Rom, ToolError> {
    FolderTool {
        name: "GPS",
        program: "gps",
        args: &["rom.sfc"],
        remove: &[],
        // GPS applies its list to the acts-like tables in place.
        owns: |rom| {
            use crate::map16::pages::{ACTS_LIKE, ACTS_LIKE_UPPER};
            let mut owned = Vec::new();
            if let Ok(at) = rom.read_u24(ACTS_LIKE) {
                owned.push(SnesAddr::new(at));
            }
            // The pointer to pages $40 on is kept less $8000.
            if let Ok(at) = rom.read_u24(ACTS_LIKE_UPPER).map(|a| a + 0x8000) {
                owned.push(SnesAddr::new(at));
            }
            owned
        },
    }
    .run(rom, tool, files, asar)
}

/// Runs AddmusicK on a copy of `rom`: in a copy of the AddmusicK folder
/// `tool`, with the project's music folder `music` laid over it and
/// Asar's library `asar` beside it, as AddmusicK wants.
pub fn addmusick(rom: &Rom, tool: &Path, music: &Path, asar: &Path) -> Result<Rom, ToolError> {
    FolderTool {
        name: "AddmusicK",
        program: "AddmusicK",
        args: &["-noblock", "rom.sfc"],
        // AddmusicK reads its options file in place of its arguments.
        remove: &["Addmusic_options.txt"],
        owns: |_| Vec::new(),
    }
    .run(rom, tool, music, asar)
}

/// Runs UberASM Tool on a copy of `rom`, in a copy of its folder `tool`
/// with the project's UberASM folder (`list.txt`, `level/`, `library/`,
/// ...) laid over it. Its release is built for 32-bit Windows; elsewhere
/// it runs as an x64 build with a native Asar (docs/toolchain.md).
pub fn uberasm(rom: &Rom, tool: &Path, files: &Path, asar: &Path) -> Result<Rom, ToolError> {
    FolderTool {
        name: "UberASM Tool",
        program: "UberASMTool",
        args: &["list.txt", "rom.sfc"],
        remove: &[],
        owns: |_| Vec::new(),
    }
    .run(rom, tool, files, asar)
}
