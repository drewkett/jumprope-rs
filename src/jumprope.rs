// This is an implementation of a Rope (fancy string) based on a skip list. This
// implementation is a rust port of librope:
// https://github.com/josephg/librope
// It does not support wide characters.

// Unlike other rust rope implementations, this implementation should be very
// fast; but it manages that through heavy use of unsafe pointers and C-style
// dynamic arrays.

// use rope::*;

use std::{mem, ptr, str};
use std::alloc::{alloc, dealloc, Layout};
use std::cmp::min;
use std::fmt::{Debug, Display, Formatter};
use std::ops::Range;
use rand::prelude::*;
use rand::Rng;
use crate::gapbuffer::GapBuffer;
use crate::utils::*;
// use crate::params::*;

// Must be <= UINT16_MAX. Benchmarking says this is pretty close to optimal
// (tested on a mac using clang 4.0 and x86_64).
//const NODE_SIZE: usize = 136;

// The likelyhood (out of 256) a node will have height (n+1) instead of n
const BIAS: u8 = 65;
// const BIAS: u8 = XX_BIAS;

// The rope will become less efficient after the string is 2 ^ ROPE_MAX_HEIGHT nodes.

#[cfg(debug_assertions)]
pub(crate) const NODE_STR_SIZE: usize = 10;
#[cfg(not(debug_assertions))]
pub(crate) const NODE_STR_SIZE: usize = 392;
// pub(crate) const NODE_STR_SIZE: usize = XX_SIZE;

const MAX_HEIGHT: usize = 20;//NODE_STR_SIZE / mem::size_of::<SkipEntry>();
const MAX_HEIGHT_U8: u8 = MAX_HEIGHT as u8;

// Using StdRng notably increases wasm code size, providing some tiny extra protection against
// ddos attacks. See main module documentation for details.
#[cfg(feature = "ddos_protection")]
type RopeRng = StdRng;
#[cfg(not(feature = "ddos_protection"))]
type RopeRng = SmallRng;


// The node structure is designed in a very fancy way which would be more at home in C or something
// like that. The basic idea is that the node structure is fixed size in memory, but the proportion
// of that space taken up by characters and by the height are different depentant on a node's
// height.
#[repr(C)]
pub struct JumpRope {
    rng: RopeRng,
    // The total number of characters in the rope
    // num_chars: usize,

    // The total number of bytes which the characters in the rope take up
    num_bytes: usize,

    // The first node is inline. The height is the max height we've ever used in the rope + 1. The
    // highest entry points "past the end" of the list, including the entire list length.
    pub(super) head: Node,

    // This is so dirty. The first node is embedded in JumpRope; but we need to allocate enough room
    // for height to get arbitrarily large. I could insist on JumpRope always getting allocated on
    // the heap, but for small strings its better that the first string is just on the stack. So
    // this struct is repr(C) and I'm just padding out the struct directly.
    nexts: [SkipEntry; MAX_HEIGHT+1],

    // The nexts array contains an extra entry at [head.height-1] the which points past the skip
    // list. The size is the size of the entire list.
}

#[repr(C)] // Prevent parameter reordering.
pub(super) struct Node {
    // The first num_bytes of this store a valid utf8 string.
    // str: [u8; NODE_STR_SIZE],
    //
    // // Number of bytes in str in use
    // num_bytes: u8,
    pub(super) str: GapBuffer<NODE_STR_SIZE>,

    // Height of nexts array.
    pub(super) height: u8,

    // #[repr(align(std::align_of::<SkipEntry>()))]

    // This array actually has the size of height; but we dynamically allocate the structure on the
    // heap to avoid wasting memory.
    // TODO: Honestly this memory saving is very small anyway. Reconsider this choice.
    nexts: [SkipEntry; 0],
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) struct SkipEntry {
    pub(super) node: *mut Node,
    /// The number of *characters* between the start of the current node and the start of the next
    /// node.
    pub(super) skip_chars: usize,
}

// Make sure nexts uses correct alignment. This should be guaranteed by repr(C)
// This test will fail if this ever stops being true.
#[test]
fn test_align() {
    #[repr(C)] struct Check([SkipEntry; 0]);
    assert!(mem::align_of::<Check>() >= mem::align_of::<SkipEntry>());
}

fn random_height(rng: &mut RopeRng) -> u8 {
    let mut h: u8 = 1;
    // TODO: This is using the thread_local rng, which is secure (?!). Check
    // this is actually fast.
    while h < MAX_HEIGHT_U8 && rng.gen::<u8>() < BIAS { h+=1; }
    h
}


impl SkipEntry {
    fn new() -> Self {
        SkipEntry { node: ptr::null_mut(), skip_chars: 0 }
    }
}

impl Node {
    pub(super) fn next_ptr(&self) -> *const Self { // TODO: Pin.
        self.first_next().node
    }

    // Do I need to be explicit about the lifetime of the references being tied
    // to the lifetime of the node?
    fn nexts(&self) -> &[SkipEntry] {
        unsafe {
            std::slice::from_raw_parts(self.nexts.as_ptr(), self.height as usize)
        }
    }

    fn nexts_mut(&mut self) -> &mut [SkipEntry] {
        unsafe {
            std::slice::from_raw_parts_mut(self.nexts.as_mut_ptr(), self.height as usize)
        }
    }

    fn layout_with_height(height: u8) -> Layout {
        Layout::from_size_align(
            mem::size_of::<Node>() + mem::size_of::<SkipEntry>() * (height as usize),
            mem::align_of::<Node>()).unwrap()
    }

    fn alloc_with_height(height: u8, content: &str) -> *mut Node {
        //println!("height {} {}", height, max_height());
        assert!(height >= 1 && height <= MAX_HEIGHT_U8);

        unsafe {
            let node = alloc(Self::layout_with_height(height)) as *mut Node;
            (*node) = Node {
                str: GapBuffer::new_from_str(content),
                height,
                nexts: [],
            };

            for next in (*node).nexts_mut() {
                *next = SkipEntry::new();
            }

            node
        }
    }

    fn alloc(rng: &mut RopeRng, content: &str) -> *mut Node {
        Self::alloc_with_height(random_height(rng), content)
    }

    unsafe fn free(p: *mut Node) {
        dealloc(p as *mut u8, Self::layout_with_height((*p).height));
    }

    fn as_str_1(&self) -> &str {
        self.str.start_as_str()
    }
    fn as_str_2(&self) -> &str {
        self.str.end_as_str()
    }

    // The height is at least 1, so this is always valid.
    pub(super) fn first_next<'a>(&self) -> &'a SkipEntry {
        unsafe { &*self.nexts.as_ptr() }
    }

    fn first_next_mut<'a>(&mut self) -> &'a mut SkipEntry {
        unsafe { &mut *self.nexts.as_mut_ptr() }
    }

    pub(super) fn num_chars(&self) -> usize {
        self.first_next().skip_chars
    }
}

#[derive(Debug, Clone)]
pub(crate) struct RopeCursor([SkipEntry; MAX_HEIGHT+1]);

impl RopeCursor {
    fn update_offsets(&mut self, height: usize, by: isize) {
        for i in 0..height {
            unsafe {
                // This is weird but makes sense when you realise the nexts in
                // the cursor are pointers into the elements that have the
                // actual pointers.
                // Also adding a usize + isize is awful in rust :/
                let skip = &mut (*self.0[i].node).nexts_mut()[i].skip_chars;
                *skip = skip.wrapping_add(by as usize);
            }
        }
    }

    fn move_within_node(&mut self, height: usize, by: isize) {
        for e in &mut self.0[..height] {
            e.skip_chars = e.skip_chars.wrapping_add(by as usize);
        }
    }

    pub(crate) fn here_ptr(&self) -> *mut Node {
        self.0[0].node
    }

    pub(crate) fn global_char_pos(&self, head_height: u8) -> usize {
        self.0[head_height as usize - 1].skip_chars
    }

    pub(crate) fn local_char_pos(&self) -> usize {
        self.0[0].skip_chars
    }
}

/// A rope is a "rich string" data structure for storing fancy strings, like the contents of a
/// text editor. See module level documentation for more information.
impl JumpRope {
    fn new_with_rng(rng: RopeRng) -> Self {
        JumpRope {
            rng,
            num_bytes: 0,
            // nexts: [SkipEntry::new(); MAX_HEIGHT],

            // We don't ever store characters in the head node, but the height
            // here is the maximum height of the entire rope.
            head: Node {
                str: GapBuffer::new(),
                height: 1,
                nexts: [],
            },
            nexts: [SkipEntry::new(); MAX_HEIGHT+1],
        }
    }

    /// Creates and returns a new, empty rope.
    ///
    /// In release mode this method is an alias for [`new_from_entropy`](Self::new_from_entropy).
    /// But when compiled for testing (or in debug mode), we use a fixed seed in order to keep tests
    /// fully deterministic.
    ///
    /// Note using this method in wasm significantly increases bundle size. Use
    /// [`new_with_seed`](Self::new_from_seed) instead.
    pub fn new() -> Self {
        if cfg!(test) || cfg!(debug_assertions) {
            Self::new_from_seed(123)
        } else {
            Self::new_from_entropy()
        }
    }

    /// Creates a new, empty rope seeded from an entropy source.
    pub fn new_from_entropy() -> Self {
        Self::new_with_rng(RopeRng::from_entropy())
    }

    /// Creates a new, empty rope using an RNG seeded from the passed u64 parameter.
    ///
    /// The performance of this library with any particular data set will vary by a few percent
    /// within a range based on the seed provided. It may be useful to fix the seed within tests or
    /// benchmarks in order to make the program entirely deterministic, though bear in mind:
    ///
    /// - Jumprope will always use a fixed seed
    pub fn new_from_seed(seed: u64) -> Self {
        Self::new_with_rng(RopeRng::seed_from_u64(seed))
    }

    fn new_from_str(s: &str) -> Self {
        let mut rope = Self::new();
        rope.insert(0, s);
        rope
    }

    /// Return the length of the rope in unicode characters. Note this is not the same as either
    /// the number of bytes the characters take, or the number of grapheme clusters in the string.
    ///
    /// This method returns the length in constant-time (*O(1)*).
    ///
    /// # Example
    ///
    /// ```
    /// # use jumprope::*;
    /// assert_eq!("↯".len(), 3);
    ///
    /// let rope = JumpRope::from("↯");
    /// assert_eq!(rope.len_chars(), 1);
    ///
    /// // The unicode snowman grapheme cluster needs 2 unicode characters.
    /// let snowman = JumpRope::from("☃️");
    /// assert_eq!(snowman.len_chars(), 2);
    /// ```
    pub fn len_chars(&self) -> usize {
        self.head.nexts()[self.head.height as usize - 1].skip_chars
    }

    // Internal function for navigating to a particular character offset in the rope.  The function
    // returns the list of nodes which point past the position, as well as offsets of how far into
    // their character lists the specified characters are.
    pub(crate) fn cursor_at_char(&self, char_pos: usize, stick_end: bool) -> RopeCursor {
        assert!(char_pos <= self.len_chars());

        let mut e: *const Node = &self.head;
        let mut height = self.head.height as usize - 1;
        
        let mut offset = char_pos; // How many more chars to skip

        let mut iter = RopeCursor([SkipEntry::new(); MAX_HEIGHT+1]);

        loop { // while height >= 0
            let en = unsafe { &*e };
            let next = en.nexts()[height];
            let skip = next.skip_chars;
            if offset > skip || (!stick_end && offset == skip && !next.node.is_null()) {
                // Go right.
                assert!(e == &self.head || !en.str.is_empty());
                offset -= skip;
                e = next.node;
                assert!(!e.is_null(), "Internal constraint violation: Reached rope end prematurely");
            } else {
                // Record this and go down.
                iter.0[height] = SkipEntry {
                    skip_chars: offset,
                    node: e as *mut Node, // This is pretty gross
                };

                if height == 0 { break; } else { height -= 1; }
            }
        };

        assert!(offset <= NODE_STR_SIZE);
        iter
    }

    fn cursor_at_start(&self) -> RopeCursor {
        RopeCursor([SkipEntry {
            node: &self.head as *const _ as *mut _,
            skip_chars: 0
        }; MAX_HEIGHT+1])
    }

    fn cursor_at_end(&self) -> RopeCursor {
        self.cursor_at_char(self.len_chars(), true)
    }

    // Internal fn to create a new node at the specified iterator filled with the specified
    // content.
    unsafe fn insert_node_at(&mut self, cursor: &mut RopeCursor, contents: &str, num_chars: usize, update_cursor: bool) {
        // println!("Insert_node_at {} len {}", contents.len(), self.num_bytes);
        // assert!(contents.len() < NODE_STR_SIZE);
        debug_assert_eq!(count_chars(contents), num_chars);
        debug_assert!(num_chars <= NODE_STR_SIZE);

        // TODO: Pin this sucka.
        // let new_node = Pin::new(Node::alloc());
        let new_node = Node::alloc(&mut self.rng, contents);
        // (*new_node).num_bytes = contents.len() as u8;
        // (*new_node).str[..contents.len()].copy_from_slice(contents.as_bytes());

        let new_height = (*new_node).height as usize;

        let mut head_height = self.head.height as usize;
        while head_height <= new_height {
            // TODO: Why do we copy here? Explain it in a comment. This is
            // currently lifted from the C code.
            self.nexts[head_height] = self.nexts[head_height - 1];
            cursor.0[head_height] = cursor.0[head_height - 1];

            self.head.height += 1; // Ends up 1 more than the max node height.
            head_height += 1;
        }

        for i in 0..new_height {
            let prev_skip = &mut (*cursor.0[i].node).nexts_mut()[i];
            let nexts = (*new_node).nexts_mut();
            nexts[i].node = prev_skip.node;
            nexts[i].skip_chars = num_chars + prev_skip.skip_chars - cursor.0[i].skip_chars;

            prev_skip.node = new_node;
            prev_skip.skip_chars = cursor.0[i].skip_chars;

            // & move the iterator to the end of the newly inserted node.
            if update_cursor {
                cursor.0[i].node = new_node;
                cursor.0[i].skip_chars = num_chars;
            }
        }

        for i in new_height..head_height {
            (*cursor.0[i].node).nexts_mut()[i].skip_chars += num_chars;
            if update_cursor {
                cursor.0[i].skip_chars += num_chars;
            }
        }

        // self.nexts[self.head.height as usize - 1].skip_chars += num_chars;
        self.num_bytes += contents.len();
    }

    unsafe fn insert_at_cursor(&mut self, cursor: &mut RopeCursor, contents: &str) {
        if contents.is_empty() { return; }
        // iter contains how far (in characters) into the current element to
        // skip. Figure out how much that is in bytes.
        let mut offset_bytes: usize = 0;
        // The insertion offset into the destination node.
        let offset: usize = cursor.0[0].skip_chars;
        let mut e = cursor.here_ptr();

        // We might be able to insert the new data into the current node, depending on
        // how big it is. We'll count the bytes, and also check that its valid utf8.
        let num_inserted_bytes = contents.len();
        let num_inserted_chars = count_chars(contents);

        // Adding this short circuit makes the code about 2% faster for 1% more code
        if (*e).str.gap_start_chars as usize == offset && (*e).str.gap_len as usize >= num_inserted_bytes {
            // Short circuit. If we can just insert all the content right here in the gap, do so.
            (*e).str.insert_in_gap(contents);
            cursor.update_offsets(self.head.height as usize, num_inserted_chars as isize);
            cursor.move_within_node(self.head.height as usize, num_inserted_chars as isize);
            self.num_bytes += num_inserted_bytes;
            return;
        }

        if offset > 0 {
            // Changing this to debug_assert reduces performance by a few % for some reason.
            assert!(offset <= (*e).nexts()[0].skip_chars);
            // This could be faster, but its not a big deal.
            offset_bytes = (*e).str.count_bytes(offset);
        }

        // Can we insert into the current node?
        let current_len_bytes = (*e).str.len_bytes();
        let mut insert_here = current_len_bytes + num_inserted_bytes <= NODE_STR_SIZE;

        // If we can't insert here, see if we can move the cursor forward and insert into the
        // subsequent node.
        if !insert_here && offset_bytes == current_len_bytes {
            // We can insert into the subsequent node if:
            // - We can't insert into the current node
            // - There _is_ a next node to insert into
            // - The insert would be at the start of the next node
            // - There's room in the next node
            if let Some(next) = (*e).first_next_mut().node.as_mut() {
                if next.str.len_bytes() + num_inserted_bytes <= NODE_STR_SIZE {
                    offset_bytes = 0;

                    // Could do this with slice::fill but this seems slightly faster.
                    for e in &mut cursor.0[..next.height as usize] {
                        *e = SkipEntry {
                            node: next,
                            skip_chars: 0
                        };
                    }
                    e = next;

                    insert_here = true;
                }
            }
        }

        if insert_here {
            // First move the current bytes later on in the string.
            let c = &mut (*e).str;
            c.try_insert(offset_bytes, contents).unwrap();

            self.num_bytes += num_inserted_bytes;
            // .... aaaand update all the offset amounts.
            cursor.update_offsets(self.head.height as usize, num_inserted_chars as isize);
            cursor.move_within_node(self.head.height as usize, num_inserted_chars as isize);
        } else {
            // There isn't room. We'll need to add at least one new node to the rope.

            // If we're not at the end of the current node, we'll need to remove
            // the end of the current node's data and reinsert it later.
            (*e).str.move_gap(offset_bytes);

            let num_end_bytes = (*e).str.len_bytes() - offset_bytes;
            let mut num_end_chars: usize = 0;
            let end_str = if num_end_bytes > 0 {
                // We'll truncate the node, but leave the bytes themselves there (for later).

                // It would also be correct (and slightly more space efficient) to pack some of the
                // new string's characters into this node after trimming it.
                let end_str = (*e).str.take_rest();
                num_end_chars = (*e).num_chars() - offset;

                cursor.update_offsets(self.head.height as usize, -(num_end_chars as isize));
                self.num_bytes -= num_end_bytes;
                Some(end_str)
            } else {
                // TODO: Don't just skip. Append as many characters as we can here.
                None
            };

            // Now we insert new nodes containing the new character data. The
            // data must be broken into pieces of with a maximum size of
            // NODE_STR_SIZE. Node boundaries must not occur in the middle of a
            // utf8 codepoint.
            // let mut str_offset: usize = 0;
            let mut remainder = contents;
            while !remainder.is_empty() {
                // println!(". {}", remainder);
                // Find the first index after STR_SIZE bytes
                let mut byte_pos = 0;
                let mut char_pos = 0;

                // Find a suitable cut point. We should take as many characters as we can fit in
                // the node, without splitting any unicode codepoints.
                for c in remainder.chars() {
                    // TODO: This could definitely be more efficient.
                    let cs = c.len_utf8();
                    if cs + byte_pos > NODE_STR_SIZE { break }
                    else {
                        char_pos += 1;
                        byte_pos += cs;
                    }
                }
                
                let (next, rem) = remainder.split_at(byte_pos);
                assert!(!next.is_empty());
                self.insert_node_at(cursor, next, char_pos, true);
                remainder = rem;
            }

            if let Some(end_str) = end_str {
                self.insert_node_at(cursor, end_str, num_end_chars, false);
            }
        }

        assert_ne!(cursor.local_char_pos(), 0);
    }

    unsafe fn del_at_cursor(&mut self, cursor: &mut RopeCursor, mut length: usize) {
        if length == 0 { return; }
        let mut offset = cursor.local_char_pos();
        let mut node = cursor.here_ptr();
        while length > 0 {
            {
                let s = (&*node).first_next();
                if offset == s.skip_chars {
                    // End of current node. Skip to the start of the next one.
                    node = s.node;
                    offset = 0;
                }
            }

            let num_chars = (&*node).num_chars();
            let removed = std::cmp::min(length, num_chars - offset);
            assert!(removed > 0);

            let height = (*node).height as usize;
            if removed < num_chars || std::ptr::eq(node, &self.head) {
                // Just trim the node down.
                let s = &mut (*node).str;
                let removed_bytes = s.remove_chars(offset, removed);
                self.num_bytes -= removed_bytes;

                for s in (*node).nexts_mut() {
                    s.skip_chars -= removed;
                }
            } else {
                // Remove the node from the skip list. This works because the cursor must be
                // pointing from the previous element to the start of this element.
                assert_ne!(cursor.0[0].node, node);

                for i in 0..(*node).height as usize {
                    let s = &mut (*cursor.0[i].node).nexts_mut()[i];
                    s.node = (*node).nexts_mut()[i].node;
                    s.skip_chars += (*node).nexts()[i].skip_chars - removed;
                }

                self.num_bytes -= (*node).str.len_bytes();
                let next = (*node).first_next().node;
                Node::free(node);
                node = next;
            }

            for i in height..self.head.height as usize {
                let s = &mut (*cursor.0[i].node).nexts_mut()[i];
                s.skip_chars -= removed;
            }

            length -= removed;
        }
    }

    fn eq_str(&self, mut other: &str) -> bool {
        if self.len_bytes() != other.len() { return false; }

        for s in self.chunks().strings() {
            let (start, rem) = other.split_at(s.len());
            if start != s { return false; }
            other = rem;
        }

        true
    }
}

impl Default for JumpRope {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for JumpRope {
    fn drop(&mut self) {
        let mut node = self.head.first_next().node;
        unsafe {
            while !node.is_null() {
                let next = (*node).first_next().node;
                Node::free(node);
                node = next;
            }
        }
    }
}

impl From<&str> for JumpRope {
    fn from(str: &str) -> Self {
        JumpRope::new_from_str(str)
    }
}

impl From<String> for JumpRope {
    fn from(str: String) -> Self {
        JumpRope::new_from_str(&str)
    }
}

impl PartialEq for JumpRope {
    // This is quite complicated. It would be cleaner to just write a bytes
    // iterator, then iterate over the bytes of both strings comparing along the
    // way.
    // However, this should be faster because it can memcmp().

    // Another way to implement this would be to rewrite it as a comparison with
    // an iterator over &str. Then the rope vs rope comparison would be trivial,
    // but also we could add comparison functions with a single &str and stuff
    // very easily.
    fn eq(&self, other: &JumpRope) -> bool {
        if self.num_bytes != other.num_bytes
                || self.len_chars() != other.len_chars() {
            return false
        }

        let mut other_iter = other.chunks().strings();

        // let mut os = other_iter.next();
        let mut os = "";

        for mut s in self.chunks().strings() {
            // Walk s.len() bytes through the other rope
            while !s.is_empty() {
                if os.is_empty() {
                    os = other_iter.next().unwrap();
                }
                debug_assert!(!os.is_empty());

                let amt = min(s.len(), os.len());
                debug_assert!(amt > 0);

                let (s_start, s_rem) = s.split_at(amt);
                let (os_start, os_rem) = os.split_at(amt);

                if s_start != os_start { return false; }

                s = s_rem;
                os = os_rem;
            }
        }

        true
    }
}
impl Eq for JumpRope {}

impl Debug for JumpRope {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.chunks().strings())
            .finish()
    }
}

impl Display for JumpRope {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        for (s, _) in self.chunks() {
            f.write_str(s)?;
        }
        Ok(())
    }
}

// I don't know why I need all three of these, but I do.
impl PartialEq<str> for JumpRope {
    fn eq(&self, other: &str) -> bool {
        self.eq_str(other)
    }
}
impl PartialEq<&str> for JumpRope {
    fn eq(&self, other: &&str) -> bool {
        self.eq_str(*other)
    }
}
impl PartialEq<String> for JumpRope {
    fn eq(&self, other: &String) -> bool {
        self.eq_str(other.as_str())
    }
}

impl<'a> Extend<&'a str> for JumpRope {
    fn extend<T: IntoIterator<Item = &'a str>>(&mut self, iter: T) {
        let mut cursor = self.cursor_at_end();
        iter.into_iter().for_each(|s| {
            unsafe { self.insert_at_cursor(&mut cursor, s); }
        });
    }
}

impl Clone for JumpRope {
    fn clone(&self) -> Self {
        // This method could be a little bit more efficient, but I think improving clone()
        // performance isn't worth the extra effort.
        let mut r = JumpRope::new();
        let mut cursor = r.cursor_at_start();
        for node in self.node_iter() {
            unsafe {
                r.insert_at_cursor(&mut cursor, node.as_str_1());
                r.insert_at_cursor(&mut cursor, node.as_str_2());
            }
        }
        r
    }
}

impl JumpRope {
    /// Insert new content into the rope. The content is inserted at the specified unicode character
    /// offset, which is different from a byte offset for non-ASCII characters.
    ///
    /// # Example
    ///
    /// ```
    /// # use jumprope::*;
    /// let mut rope = JumpRope::from("--");
    /// rope.insert(1, "hi there");
    /// assert_eq!(rope.to_string(), "-hi there-");
    /// ```
    ///
    /// If the position names a location past the end of the rope, it is truncated.
    pub fn insert(&mut self, mut pos: usize, contents: &str) {
        if contents.is_empty() { return; }
        pos = std::cmp::min(pos, self.len_chars());

        let mut cursor = self.cursor_at_char(pos, true);
        unsafe { self.insert_at_cursor(&mut cursor, contents); }

        debug_assert_eq!(cursor.global_char_pos(self.head.height), pos + count_chars(contents));
        // dbg!(&cursor.0[..self.head.height as usize]);
    }

    /// Delete a span of unicode characters from the rope. The span is specified in unicode
    /// characters, not bytes.
    ///
    /// Any attempt to delete past the end of the rope will be silently ignored.
    ///
    /// # Example
    ///
    /// ```
    /// # use jumprope::*;
    /// let mut rope = JumpRope::from("Whoa dawg!");
    /// rope.remove(4..9); // delete " dawg"
    /// assert_eq!(rope.to_string(), "Whoa!");
    /// ```
    pub fn remove(&mut self, mut range: Range<usize>) {
        range.end = range.end.min(self.len_chars());
        if range.start >= range.end { return; }

        // We need to stick_end so we can delete entries.
        let mut cursor = self.cursor_at_char(range.start, true);
        unsafe { self.del_at_cursor(&mut cursor, range.end - range.start); }

        debug_assert_eq!(cursor.global_char_pos(self.head.height), range.start);
    }

    /// Replace the specified range with new content. This is equivalent to calling
    /// [`remove`](Self::remove) followed by [`insert`](Self::insert), but it is simpler and faster.
    ///
    /// # Example
    ///
    /// ```
    /// # use jumprope::*;
    /// let mut rope = JumpRope::from("Hi Mike!");
    /// rope.replace(3..7, "Duane"); // replace "Mike" with "Duane"
    /// assert_eq!(rope.to_string(), "Hi Duane!");
    /// ```
    pub fn replace(&mut self, range: Range<usize>, content: &str) {
        let len = self.len_chars();
        let pos = usize::min(range.start, len);
        let del_len = usize::min(range.end, len) - pos;

        let mut cursor = self.cursor_at_char(pos, true);
        if del_len > 0 {
            unsafe { self.del_at_cursor(&mut cursor, del_len); }
        }
        if !content.is_empty() {
            unsafe { self.insert_at_cursor(&mut cursor, content); }
        }

        debug_assert_eq!(cursor.global_char_pos(self.head.height), pos + count_chars(content));
    }

    /// Get the number of bytes used for the UTF8 representation of the rope. This will always match
    /// the .len() property of the equivalent String.
    ///
    /// Note: This is only useful in specific situations - like preparing a byte buffer for saving
    /// or sending over the internet. In many cases it is preferable to use
    /// [`len_chars`](Self::len_chars).
    ///
    /// # Example
    ///
    /// ```
    /// # use jumprope::*;
    /// let str = "κόσμε"; // "Cosmos" in ancient greek
    /// assert_eq!(str.len(), 11); // 11 bytes over the wire
    ///
    /// let rope = JumpRope::from(str);
    /// assert_eq!(rope.len_bytes(), str.len());
    /// ```
    pub fn len_bytes(&self) -> usize { self.num_bytes }

    /// Returns `true` if the rope contains no elements.
    pub fn is_empty(&self) -> bool { self.num_bytes == 0 }

    pub fn check(&self) {
        assert!(self.head.height >= 1);
        assert!(self.head.height < MAX_HEIGHT_U8 + 1);

        let skip_over = &self.nexts[self.head.height as usize - 1];
        // println!("Skip over skip chars {}, num bytes {}", skip_over.skip_chars, self.num_bytes);
        assert!(skip_over.skip_chars <= self.num_bytes as usize);
        assert!(skip_over.node.is_null());

        // The offsets store the total distance travelled since the start.
        let mut iter = [SkipEntry::new(); MAX_HEIGHT];
        for i in 0..self.head.height {
            // Bleh.
            iter[i as usize].node = &self.head as *const Node as *mut Node;
        }

        let mut num_bytes: usize = 0;
        let mut num_chars = 0;

        for n in self.node_iter() {
            // println!("visiting {:?}", n.as_str());
            assert!(!n.str.is_empty() || std::ptr::eq(n, &self.head));
            assert!(n.height <= MAX_HEIGHT_U8);
            assert!(n.height >= 1);
            n.str.check();

            assert_eq!(count_chars(n.as_str_1()) + count_chars(n.as_str_2()), n.num_chars());
            for (i, entry) in iter[0..n.height as usize].iter_mut().enumerate() {
                assert_eq!(entry.node as *const Node, n as *const Node);
                assert_eq!(entry.skip_chars, num_chars);

                // println!("replacing entry {:?} with {:?}", entry, n.nexts()[i].node);
                entry.node = n.nexts()[i].node;
                entry.skip_chars += n.nexts()[i].skip_chars;
            }

            num_bytes += n.str.len_bytes();
            num_chars += n.num_chars();
        }

        for entry in iter[0..self.head.height as usize].iter() {
            // println!("{:?}", entry);
            assert!(entry.node.is_null());
            assert_eq!(entry.skip_chars, num_chars);
        }

        // println!("self bytes: {}, count bytes {}", self.num_bytes, num_bytes);
        assert_eq!(self.num_bytes, num_bytes);
        assert_eq!(self.len_chars(), num_chars);
    }

    /// This method counts the number of bytes of memory allocated in the rope. This is purely for
    /// debugging.
    ///
    /// Notes:
    ///
    /// - This method (its existence, its signature and its return value) is not considered part of
    ///   the stable API provided by jumprope. This may disappear or change in point releases.
    /// - This method walks the entire rope. It has time complexity O(n).
    /// - If a rope is owned inside another structure, this method will double-count the bytes
    ///   stored in the rope's head.
    pub fn mem_size(&self) -> usize {
        let mut nodes = self.node_iter();
        let mut size = 0;
        // The first node is the head. Count the actual head size.
        size += std::mem::size_of::<Self>();
        nodes.next(); // And discard it from the iterator.

        for n in nodes {
            let layout = Node::layout_with_height(n.height);
            size += layout.size();
        }

        size
    }

    #[allow(unused)]
    pub(crate) fn print(&self) {
        println!("chars: {}\tbytes: {}\theight: {}", self.len_chars(), self.num_bytes, self.head.height);

        print!("HEAD:");
        for s in self.head.nexts() {
            print!(" |{} ", s.skip_chars);
        }
        println!();

        for (i, node) in self.node_iter().enumerate() {
            print!("{}:", i);
            for s in node.nexts() {
                print!(" |{} ", s.skip_chars);
            }
            println!("      : {:?} + {:?}", node.as_str_1(), node.as_str_2());
        }
    }
}
