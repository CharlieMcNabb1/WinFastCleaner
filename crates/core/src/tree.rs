//! Compact in-memory folder tree produced by every scan method.
//!
//! A full scan of a large drive can produce millions of entries, so the tree
//! is stored as flat arrays instead of a pointer-based structure:
//!
//! - every node lives in one `Vec<Node>`, addressed by a [`NodeId`],
//! - all names share a single byte buffer,
//! - children of a node are a contiguous range in one `Vec<NodeId>`, sorted
//!   by size (largest first).
//!
//! Scanners add raw entries to a [`TreeBuilder`] in any order and then call
//! [`TreeBuilder::finish`], which drops unreachable entries, counts hard links
//! once, aggregates folder sizes, and sorts children.

use std::borrow::Cow;
use std::collections::HashSet;
use std::ffi::{OsStr, OsString};
use std::path::{Component, Path, PathBuf};

/// Index of a node in a [`Tree`] or [`TreeBuilder`].
pub type NodeId = u32;

/// Marker for "no node" (for example, the parent of the root).
pub const NO_NODE: NodeId = u32::MAX;

/// Bit flags stored on every node.
pub mod flags {
    /// The node is a directory.
    pub const DIR: u16 = 1 << 0;
    /// Symlink, junction, or mount point. Scanners never follow these.
    pub const LINK: u16 = 1 << 1;
    /// A file with more than one name (hard link).
    pub const HARDLINK: u16 = 1 << 2;
    /// Another name for a hard-linked file that is already counted elsewhere
    /// in this tree. Its size is reported as zero so the total stays correct.
    pub const HARDLINK_DUP: u16 = 1 << 3;
    /// A folder that contains hard-linked files. Each one is counted once, so
    /// File Explorer (which counts every name) may show a larger size.
    pub const HAS_HARDLINKS: u16 = 1 << 4;
    /// A folder that could not be read (access denied, in use, and so on).
    pub const UNREADABLE: u16 = 1 << 5;
    /// NTFS metadata such as `$MFT` or `$LogFile`.
    pub const METAFILE: u16 = 1 << 6;
    /// A cloud placeholder whose contents are not stored on this disk.
    pub const CLOUD: u16 = 1 << 7;
    /// Hidden attribute.
    pub const HIDDEN: u16 = 1 << 8;
    /// System attribute.
    pub const SYSTEM: u16 = 1 << 9;
    /// The folder contains at least one unreadable folder, so its size may be
    /// incomplete.
    pub const HAS_UNREADABLE: u16 = 1 << 10;
}

const NO_GROUP: u32 = u32::MAX;

/// One file or folder in a finished [`Tree`].
#[derive(Clone, Debug)]
pub struct Node {
    name_off: u32,
    name_len: u32,
    parent: NodeId,
    child_start: u32,
    child_len: u32,
    flags: u16,
    size: u64,
    allocated: u64,
    files: u64,
    dirs: u64,
    modified: i64,
}

impl Node {
    /// Logical size in bytes. For folders, the sum of everything inside,
    /// with each hard-linked file counted once.
    pub fn size(&self) -> u64 {
        self.size
    }

    /// Size on disk in bytes (allocated clusters).
    pub fn allocated(&self) -> u64 {
        self.allocated
    }

    /// Number of files at or below this node (1 for a plain file).
    pub fn files(&self) -> u64 {
        self.files
    }

    /// Number of folders below this node.
    pub fn dirs(&self) -> u64 {
        self.dirs
    }

    /// Last-modified time as Unix seconds. For folders, the newest time of
    /// the folder itself and everything inside it. Zero when unknown.
    pub fn modified(&self) -> i64 {
        self.modified
    }

    /// Raw flag bits (see [`flags`]).
    pub fn flags(&self) -> u16 {
        self.flags
    }

    /// Whether the given flag bit is set.
    pub fn has(&self, flag: u16) -> bool {
        self.flags & flag != 0
    }

    /// Whether this node is a directory.
    pub fn is_dir(&self) -> bool {
        self.has(flags::DIR)
    }

    /// Number of direct children.
    pub fn child_count(&self) -> usize {
        self.child_len as usize
    }
}

/// Data for a raw entry passed to [`TreeBuilder::push`].
#[derive(Clone, Copy, Debug)]
pub struct NodeData {
    pub parent: NodeId,
    pub flags: u16,
    pub size: u64,
    pub allocated: u64,
    pub modified: i64,
    /// Identity of the underlying file when it has several names. All names
    /// with the same group are counted once.
    pub link_group: Option<u32>,
}

#[derive(Clone, Debug)]
struct RawNode {
    name_off: u32,
    name_len: u32,
    parent: NodeId,
    flags: u16,
    size: u64,
    allocated: u64,
    modified: i64,
    link_group: u32,
}

/// Accumulates raw entries from a scanner, then produces a [`Tree`].
#[derive(Debug, Default)]
pub struct TreeBuilder {
    // Invariant: only ever contains bytes from `OsStr::as_encoded_bytes` or
    // valid UTF-8, so `Tree::name_os` may rebuild `OsStr`s from it.
    names: Vec<u8>,
    nodes: Vec<RawNode>,
}

impl TreeBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(nodes: usize, name_bytes: usize) -> Self {
        Self {
            names: Vec::with_capacity(name_bytes),
            nodes: Vec::with_capacity(nodes),
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Adds an entry and returns its id. The parent may be added later and
    /// linked with [`TreeBuilder::set_parent`].
    pub fn push(&mut self, name: &OsStr, node: NodeData) -> NodeId {
        let (name_off, name_len) = self.store_name(name.as_encoded_bytes());
        self.push_raw(name_off, name_len, &node)
    }

    /// Adds an entry whose name is UTF-16 (as stored by NTFS) without an
    /// intermediate allocation in the common case.
    pub fn push_utf16(&mut self, name: &[u16], node: NodeData) -> NodeId {
        let start = self.names.len();
        let mut valid = true;
        for c in char::decode_utf16(name.iter().copied()) {
            match c {
                Ok(c) => {
                    let mut buf = [0u8; 4];
                    self.names
                        .extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
                }
                Err(_) => {
                    valid = false;
                    break;
                }
            }
        }
        if !valid {
            // Unpaired surrogates: let the standard library encode the name.
            self.names.truncate(start);
            let os = os_from_utf16(name);
            self.names.extend_from_slice(os.as_encoded_bytes());
        }
        let (name_off, name_len) = (start as u32, (self.names.len() - start) as u32);
        self.push_raw(name_off, name_len, &node)
    }

    fn store_name(&mut self, bytes: &[u8]) -> (u32, u32) {
        let off = self.names.len();
        self.names.extend_from_slice(bytes);
        (off as u32, bytes.len() as u32)
    }

    fn push_raw(&mut self, name_off: u32, name_len: u32, node: &NodeData) -> NodeId {
        let id = self.nodes.len() as NodeId;
        assert!(id != NO_NODE, "tree is full");
        self.nodes.push(RawNode {
            name_off,
            name_len,
            parent: node.parent,
            flags: node.flags,
            size: node.size,
            allocated: node.allocated,
            modified: node.modified,
            link_group: node.link_group.unwrap_or(NO_GROUP),
        });
        id
    }

    pub fn set_parent(&mut self, id: NodeId, parent: NodeId) {
        self.nodes[id as usize].parent = parent;
    }

    /// Replaces everything but the name of an entry.
    pub fn set_data(&mut self, id: NodeId, node: NodeData) {
        let raw = &mut self.nodes[id as usize];
        raw.parent = node.parent;
        raw.flags = node.flags;
        raw.size = node.size;
        raw.allocated = node.allocated;
        raw.modified = node.modified;
        raw.link_group = node.link_group.unwrap_or(NO_GROUP);
    }

    pub fn add_flags(&mut self, id: NodeId, flags: u16) {
        self.nodes[id as usize].flags |= flags;
    }

    fn raw_name(&self, id: usize) -> &[u8] {
        let n = &self.nodes[id];
        &self.names[n.name_off as usize..(n.name_off + n.name_len) as usize]
    }

    /// Finishes the tree rooted at `root`. `root_path` is the real path of
    /// that root on disk.
    pub fn finish(self, root: NodeId, root_path: PathBuf) -> Tree {
        self.finish_at(root, &[], root_path)
            .expect("finishing at the root always succeeds")
    }

    /// Finishes the subtree found by following `components` (folder names,
    /// matched case-insensitively) down from `root`. Returns `None` if the
    /// path does not exist or passes through a link.
    pub fn finish_at(
        self,
        root: NodeId,
        components: &[&OsStr],
        root_path: PathBuf,
    ) -> Option<Tree> {
        let n = self.nodes.len();
        if root as usize >= n {
            return None;
        }

        // 1. Children lists (CSR layout) from parent pointers. The scan root
        //    is never anyone's child, which also breaks any self-reference.
        let mut counts = vec![0u32; n + 1];
        for (i, node) in self.nodes.iter().enumerate() {
            let p = node.parent;
            if i != root as usize && (p as usize) < n && p as usize != i {
                counts[p as usize + 1] += 1;
            }
        }
        for i in 1..=n {
            counts[i] += counts[i - 1];
        }
        let offsets = counts;
        let mut fill = offsets.clone();
        let mut kids = vec![0 as NodeId; offsets[n] as usize];
        for (i, node) in self.nodes.iter().enumerate() {
            let p = node.parent;
            if i != root as usize && (p as usize) < n && p as usize != i {
                kids[fill[p as usize] as usize] = i as NodeId;
                fill[p as usize] += 1;
            }
        }
        // Deterministic order (by name) so hard-link attribution does not
        // depend on scan timing.
        for p in 0..n {
            let range = offsets[p] as usize..offsets[p + 1] as usize;
            if range.len() > 1 {
                kids[range].sort_unstable_by(|&a, &b| {
                    self.raw_name(a as usize).cmp(self.raw_name(b as usize))
                });
            }
        }
        let raw_children = |id: usize| &kids[offsets[id] as usize..offsets[id + 1] as usize];

        // 2. Resolve the requested sub-path.
        let mut start = root as usize;
        for comp in components {
            let want = comp.to_string_lossy();
            let found = raw_children(start).iter().copied().find(|&c| {
                let raw = &self.nodes[c as usize];
                raw.flags & flags::DIR != 0
                    && raw.flags & flags::LINK == 0
                    && names_equal_ignore_case(
                        &String::from_utf8_lossy(self.raw_name(c as usize)),
                        &want,
                    )
            })?;
            start = found as usize;
        }

        // 3. Pre-order walk of everything reachable from `start`.
        let mut order: Vec<u32> = Vec::with_capacity(n);
        let mut visited = vec![false; n];
        let mut stack = vec![start as u32];
        while let Some(id) = stack.pop() {
            if std::mem::replace(&mut visited[id as usize], true) {
                continue;
            }
            order.push(id);
            // Push in reverse so children come out in name order.
            for &c in raw_children(id as usize).iter().rev() {
                if !visited[c as usize] {
                    stack.push(c);
                }
            }
        }

        // 4. Copy reachable nodes in pre-order, counting each hard-linked
        //    file only at the first name we meet.
        let mut new_id = vec![NO_NODE; n];
        for (i, &old) in order.iter().enumerate() {
            new_id[old as usize] = i as NodeId;
        }
        let mut seen_groups: HashSet<u32> = HashSet::new();
        let mut nodes: Vec<Node> = Vec::with_capacity(order.len());
        let mut groups: Vec<u32> = Vec::with_capacity(order.len());
        for &old in &order {
            let raw = &self.nodes[old as usize];
            let mut node = Node {
                name_off: raw.name_off,
                name_len: raw.name_len,
                parent: if old as usize == start {
                    NO_NODE
                } else {
                    new_id[raw.parent as usize]
                },
                child_start: 0,
                child_len: 0,
                flags: raw.flags,
                size: raw.size,
                allocated: raw.allocated,
                files: 0,
                dirs: 0,
                modified: raw.modified,
            };
            if raw.flags & flags::DIR == 0 {
                if raw.flags & flags::LINK == 0 {
                    node.files = 1;
                }
                if raw.link_group != NO_GROUP {
                    node.flags |= flags::HARDLINK;
                    if !seen_groups.insert(raw.link_group) {
                        node.flags |= flags::HARDLINK_DUP;
                        node.size = 0;
                        node.allocated = 0;
                    }
                }
            } else {
                // Folders' own sizes are recomputed from their contents.
                node.size = 0;
                node.allocated = 0;
            }
            groups.push(raw.link_group);
            nodes.push(node);
        }

        // 5. Children ranges in the new numbering.
        let mut children: Vec<NodeId> = Vec::with_capacity(order.len().saturating_sub(1));
        for (i, &old) in order.iter().enumerate() {
            let start_idx = children.len() as u32;
            for &c in raw_children(old as usize) {
                let nc = new_id[c as usize];
                if nc != NO_NODE && nodes[nc as usize].parent == i as NodeId {
                    children.push(nc);
                }
            }
            nodes[i].child_start = start_idx;
            nodes[i].child_len = children.len() as u32 - start_idx;
        }

        // 6. Aggregate bottom-up (reverse pre-order visits children first).
        for i in (1..nodes.len()).rev() {
            let (size, allocated, files, dirs, modified, fl) = {
                let c = &nodes[i];
                (c.size, c.allocated, c.files, c.dirs, c.modified, c.flags)
            };
            let p = nodes[i].parent as usize;
            let parent = &mut nodes[p];
            parent.size += size;
            parent.allocated += allocated;
            parent.files += files;
            parent.dirs += dirs + u64::from(fl & flags::DIR != 0);
            parent.modified = parent.modified.max(modified);
            if fl & (flags::HARDLINK | flags::HAS_HARDLINKS) != 0 {
                parent.flags |= flags::HAS_HARDLINKS;
            }
            if fl & (flags::UNREADABLE | flags::HAS_UNREADABLE) != 0 {
                parent.flags |= flags::HAS_UNREADABLE;
            }
        }

        // 7. Largest first; ties broken by name for stable output.
        for i in 0..nodes.len() {
            let (s, l) = (nodes[i].child_start as usize, nodes[i].child_len as usize);
            if l > 1 {
                let mut range = children[s..s + l].to_vec();
                range.sort_by(|&a, &b| {
                    let (na, nb) = (&nodes[a as usize], &nodes[b as usize]);
                    nb.size.cmp(&na.size).then_with(|| {
                        self.names[na.name_off as usize..(na.name_off + na.name_len) as usize].cmp(
                            &self.names[nb.name_off as usize..(nb.name_off + nb.name_len) as usize],
                        )
                    })
                });
                children[s..s + l].copy_from_slice(&range);
            }
        }

        let mut root_path = root_path;
        for comp in components {
            root_path.push(comp);
        }
        Some(Tree {
            root_path,
            names: self.names,
            nodes,
            children,
        })
    }
}

/// A finished, read-only scan tree. The root is always node 0.
#[derive(Debug)]
pub struct Tree {
    root_path: PathBuf,
    names: Vec<u8>,
    nodes: Vec<Node>,
    children: Vec<NodeId>,
}

impl Tree {
    pub fn root(&self) -> NodeId {
        0
    }

    /// The real path of the root folder.
    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Returns the node, or `None` if the id is out of range.
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id as usize)
    }

    /// Returns the node. Panics if the id is out of range.
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn parent(&self, id: NodeId) -> Option<NodeId> {
        let p = self.node(id).parent;
        (p != NO_NODE).then_some(p)
    }

    /// Children sorted by size, largest first.
    pub fn children(&self, id: NodeId) -> &[NodeId] {
        let n = self.node(id);
        &self.children[n.child_start as usize..(n.child_start + n.child_len) as usize]
    }

    /// The exact name as the OS reports it.
    pub fn name_os(&self, id: NodeId) -> &OsStr {
        if id == self.root() {
            return self.root_path.as_os_str();
        }
        let n = self.node(id);
        let bytes = &self.names[n.name_off as usize..(n.name_off + n.name_len) as usize];
        // SAFETY: `TreeBuilder` only stores bytes produced by
        // `OsStr::as_encoded_bytes` or valid UTF-8, and each node's range
        // covers exactly one such name.
        unsafe { OsStr::from_encoded_bytes_unchecked(bytes) }
    }

    /// The name for display (invalid Unicode replaced).
    pub fn name(&self, id: NodeId) -> Cow<'_, str> {
        self.name_os(id).to_string_lossy()
    }

    /// Full path of a node.
    pub fn path(&self, id: NodeId) -> PathBuf {
        let mut chain = Vec::new();
        let mut cur = id;
        while cur != self.root() {
            chain.push(cur);
            match self.parent(cur) {
                Some(p) => cur = p,
                None => break,
            }
        }
        let mut path = self.root_path.clone();
        for &c in chain.iter().rev() {
            path.push(self.name_os(c));
        }
        path
    }

    /// Ancestors from the root down to (and including) `id`.
    pub fn lineage(&self, id: NodeId) -> Vec<NodeId> {
        let mut chain = vec![id];
        let mut cur = id;
        while let Some(p) = self.parent(cur) {
            chain.push(p);
            cur = p;
        }
        chain.reverse();
        chain
    }

    /// Finds the node for a path inside this tree (case-insensitive).
    pub fn find(&self, path: &Path) -> Option<NodeId> {
        let rel = strip_prefix_ignore_case(path, &self.root_path)?;
        let mut cur = self.root();
        for comp in rel.components() {
            let Component::Normal(name) = comp else {
                return None;
            };
            let want = name.to_string_lossy();
            cur = self
                .children(cur)
                .iter()
                .copied()
                .find(|&c| names_equal_ignore_case(&self.name(c), &want))?;
        }
        Some(cur)
    }
}

fn names_equal_ignore_case(a: &str, b: &str) -> bool {
    a == b
        || a.chars()
            .flat_map(char::to_lowercase)
            .eq(b.chars().flat_map(char::to_lowercase))
}

fn strip_prefix_ignore_case<'a>(path: &'a Path, prefix: &Path) -> Option<&'a Path> {
    if let Ok(rel) = path.strip_prefix(prefix) {
        return Some(rel);
    }
    let mut pc = path.components();
    for want in prefix.components() {
        let got = pc.next()?;
        if !names_equal_ignore_case(
            &got.as_os_str().to_string_lossy(),
            &want.as_os_str().to_string_lossy(),
        ) {
            return None;
        }
    }
    Some(pc.as_path())
}

#[cfg(windows)]
fn os_from_utf16(units: &[u16]) -> OsString {
    use std::os::windows::ffi::OsStringExt;
    OsString::from_wide(units)
}

#[cfg(not(windows))]
fn os_from_utf16(units: &[u16]) -> OsString {
    OsString::from(String::from_utf16_lossy(units))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(parent: NodeId, size: u64) -> NodeData {
        NodeData {
            parent,
            flags: 0,
            size,
            allocated: size.div_ceil(4096) * 4096,
            modified: 100,
            link_group: None,
        }
    }

    fn dir(parent: NodeId) -> NodeData {
        NodeData {
            parent,
            flags: flags::DIR,
            size: 0,
            allocated: 0,
            modified: 1,
            link_group: None,
        }
    }

    #[test]
    fn aggregates_sizes_and_sorts_children() {
        let mut b = TreeBuilder::new();
        let root = b.push(OsStr::new("root"), dir(NO_NODE));
        let a = b.push(OsStr::new("a"), dir(root));
        b.push(OsStr::new("small.txt"), file(root, 10));
        b.push(OsStr::new("big.bin"), file(a, 5000));
        b.push(OsStr::new("mid.bin"), file(a, 300));
        let t = b.finish(root, PathBuf::from("/x"));

        assert_eq!(t.node(0).size(), 5310);
        assert_eq!(t.node(0).files(), 3);
        assert_eq!(t.node(0).dirs(), 1);
        let kids: Vec<_> = t
            .children(0)
            .iter()
            .map(|&c| t.name(c).into_owned())
            .collect();
        assert_eq!(kids, ["a", "small.txt"]);
        let a_id = t.children(0)[0];
        let a_kids: Vec<_> = t
            .children(a_id)
            .iter()
            .map(|&c| t.name(c).into_owned())
            .collect();
        assert_eq!(a_kids, ["big.bin", "mid.bin"]);
        assert_eq!(t.path(t.children(a_id)[0]), PathBuf::from("/x/a/big.bin"));
        assert_eq!(t.node(0).modified(), 100);
    }

    #[test]
    fn hard_links_are_counted_once() {
        let mut b = TreeBuilder::new();
        let root = b.push(OsStr::new("r"), dir(NO_NODE));
        let a = b.push(OsStr::new("a"), dir(root));
        let z = b.push(OsStr::new("z"), dir(root));
        b.push(
            OsStr::new("f"),
            NodeData {
                link_group: Some(7),
                ..file(a, 1000)
            },
        );
        b.push(
            OsStr::new("g"),
            NodeData {
                link_group: Some(7),
                ..file(z, 1000)
            },
        );
        let t = b.finish(root, PathBuf::from("/r"));
        assert_eq!(t.node(0).size(), 1000);
        assert_eq!(t.node(0).files(), 2);
        assert!(t.node(0).has(flags::HAS_HARDLINKS));
        let a_id = t.find(Path::new("/r/a")).unwrap();
        let z_id = t.find(Path::new("/r/z")).unwrap();
        // The first name in name order ("a" < "z") carries the size.
        assert_eq!(t.node(a_id).size(), 1000);
        assert_eq!(t.node(z_id).size(), 0);
        let g = t.children(z_id)[0];
        assert!(t.node(g).has(flags::HARDLINK_DUP));
    }

    #[test]
    fn unreachable_and_cyclic_entries_are_dropped() {
        let mut b = TreeBuilder::new();
        let root = b.push(OsStr::new("r"), dir(NO_NODE));
        b.set_parent(root, root); // self-reference, as the NTFS root has
        b.push(OsStr::new("ok"), file(root, 1));
        let x = b.push(OsStr::new("x"), dir(NO_NODE));
        let y = b.push(OsStr::new("y"), dir(x));
        b.set_parent(x, y); // cycle not connected to the root
        b.push(OsStr::new("orphan"), file(99_999, 50));
        let t = b.finish(root, PathBuf::from("/r"));
        assert_eq!(t.len(), 2);
        assert_eq!(t.node(0).size(), 1);
    }

    #[test]
    fn finish_at_subpath_is_case_insensitive_and_skips_links() {
        let mut b = TreeBuilder::new();
        let root = b.push(OsStr::new("C:"), dir(NO_NODE));
        let users = b.push(OsStr::new("Users"), dir(root));
        let me = b.push(OsStr::new("Me"), dir(users));
        b.push(OsStr::new("f"), file(me, 42));
        b.push(
            OsStr::new("Link"),
            NodeData {
                flags: flags::DIR | flags::LINK,
                ..dir(users)
            },
        );
        let t = b
            .finish_at(
                root,
                &[OsStr::new("users"), OsStr::new("ME")],
                PathBuf::from("/c"),
            )
            .unwrap();
        assert_eq!(t.node(0).size(), 42);
        assert_eq!(t.root_path(), Path::new("/c/users/ME"));

        let mut b = TreeBuilder::new();
        let root = b.push(OsStr::new("C:"), dir(NO_NODE));
        b.push(
            OsStr::new("Link"),
            NodeData {
                flags: flags::DIR | flags::LINK,
                ..dir(root)
            },
        );
        assert!(
            b.finish_at(root, &[OsStr::new("Link")], PathBuf::from("/c"))
                .is_none()
        );
    }

    #[test]
    fn utf16_names_round_trip() {
        let mut b = TreeBuilder::new();
        let root = b.push(OsStr::new("r"), dir(NO_NODE));
        let name: Vec<u16> = "héllo wörld 📁".encode_utf16().collect();
        b.push_utf16(&name, file(root, 1));
        let t = b.finish(root, PathBuf::from("/r"));
        assert_eq!(t.name(t.children(0)[0]), "héllo wörld 📁");
    }
}
