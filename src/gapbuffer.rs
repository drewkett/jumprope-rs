
use crate::utils::*;

#[derive(Debug, Clone, Eq)]
pub struct GapBuffer<const LEN: usize> {
    data: [u8; LEN],

    pub(crate) gap_start_bytes: u16,
    pub(crate) gap_start_chars: u16,
    pub(crate) gap_len: u16,
    all_ascii: bool,
}

#[inline]
unsafe fn slice_to_str(arr: &[u8]) -> &str {
    if cfg!(debug_assertions) {
        std::str::from_utf8(arr).unwrap()
    } else {
        std::str::from_utf8_unchecked(arr)
    }
}

impl<const LEN: usize> GapBuffer<LEN> {
    pub fn new() -> Self {
        Self {
            data: [0; LEN],
            gap_start_bytes: 0,
            gap_start_chars: 0,
            gap_len: LEN as u16,
            all_ascii: true,
        }
    }

    pub fn new_from_str(s: &str) -> Self {
        let mut val = Self::new();
        val.try_insert(0, s).unwrap();
        val
    }

    #[allow(unused)]
    pub fn len_space(&self) -> usize {
        self.gap_len as usize
    }

    /// In bytes.
    pub fn len_bytes(&self) -> usize {
        LEN - self.gap_len as usize
    }

    // #[allow(unused)]
    // pub fn char_len(&self) -> usize {
    //     count_chars(self.start_as_str()) + count_chars(self.end_as_str())
    // }

    pub fn is_empty(&self) -> bool {
        self.gap_len as usize == LEN
    }

    fn count_internal_chars(&self, s: &str) -> usize {
        if self.all_ascii { s.len() } else { count_chars(s) }
    }

    fn int_str_get_byte_offset(&self, s: &str, char_pos: usize) -> usize {
        if self.all_ascii { char_pos } else { str_chars_to_bytes(s, char_pos) }
    }
    fn int_chars_to_bytes_backwards(&self, s: &str, char_len: usize) -> usize {
        if self.all_ascii { char_len } else { str_chars_to_bytes_rev(s, char_len) }
    }

    pub fn move_gap(&mut self, new_start: usize) {
        let current_start = self.gap_start_bytes as usize;

        if new_start != current_start {
            let len = self.gap_len as usize;
            debug_assert!(new_start <= LEN-len);

            if new_start < current_start {
                // move characters to the right.
                let moved_chars = new_start..current_start;
                let char_len = self.count_internal_chars(unsafe { slice_to_str(&self.data[moved_chars.clone()]) });
                self.gap_start_chars -= char_len as u16;

                self.data.copy_within(moved_chars, new_start + len);
            } else if current_start < new_start {
                // Move characters to the left
                let moved_chars = current_start+len..new_start+len;
                let char_len = self.count_internal_chars(unsafe { slice_to_str(&self.data[moved_chars.clone()]) });
                self.gap_start_chars += char_len as u16;

                self.data.copy_within(moved_chars, current_start);
            }

            if cfg!(debug_assertions) {
                // This is unnecessary but tidy, and makes debugging easier.
                self.data[new_start..new_start+len].fill(0);
            }

            self.gap_start_bytes = new_start as u16;
        }
    }

    /// Panics if there's no room.
    pub fn insert_in_gap(&mut self, s: &str) {
        let len = s.len();
        let char_len = count_chars(s);
        assert!(len <= self.gap_len as usize);

        let start = self.gap_start_bytes as usize;
        self.data[start..start+len].copy_from_slice(s.as_bytes());
        self.gap_start_bytes += len as u16;
        self.gap_start_chars += char_len as u16;
        self.gap_len -= len as u16;

        if len != char_len { self.all_ascii = false; }
    }

    pub fn try_insert(&mut self, byte_pos: usize, s: &str) -> Result<(), ()> {
        let len = s.len();
        if len > self.gap_len as usize {
            // No space in this node!
            Result::Err(())
        } else {
            self.move_gap(byte_pos);
            self.insert_in_gap(s);
            Result::Ok(())
        }
    }

    pub fn remove_at_gap(&mut self, del_len: usize) {
        if cfg!(debug_assertions) {
            // Zero out the deleted bytes in debug mode.
            self.data[
                (self.gap_start_bytes +self.gap_len) as usize..(self.gap_start_bytes +self.gap_len) as usize + del_len
                ].fill(0);
        }
        self.gap_len += del_len as u16;
    }

    // Returns the number of items actually removed.
    #[allow(unused)]
    pub fn remove(&mut self, pos: usize, del_len: usize) -> usize {
        let len = self.len_bytes();

        if pos >= len { return 0; }
        let del_len = del_len.min(len - pos);

        self.move_gap(pos);

        self.remove_at_gap(del_len);
        del_len
    }

    pub fn remove_chars(&mut self, pos: usize, mut del_len: usize) -> usize {
        // This function is longer than it needs to be; but having it be a bit longer makes the
        // code faster. I think the trade-off is worth it.
        // self.move_gap(self.count_bytes(pos));
        // let removed_bytes = str_get_byte_offset(s.end_as_str(), del_len);
        // self.remove_at_gap(removed_bytes);
        // removed_bytes

        if del_len == 0 { return 0; }
        debug_assert!(del_len <= self.len_bytes() - pos);
        let mut rm_start_bytes = 0;

        let gap_chars = self.gap_start_chars as usize;
        if pos <= gap_chars && pos+del_len >= gap_chars {
            if pos < gap_chars {
                // Delete the bit from pos..gap.
                // TODO: It would be better to count backwards here.
                // let pos_bytes = str_get_byte_offset(self.start_as_str(), pos) as u16;
                // rm_start_bytes = self.gap_start_bytes - pos_bytes;
                rm_start_bytes = self.int_chars_to_bytes_backwards(self.start_as_str(), gap_chars - pos) as u16;

                del_len -= self.gap_start_chars as usize - pos;
                self.gap_len += rm_start_bytes;
                self.gap_start_chars = pos as u16;
                self.gap_start_bytes -= rm_start_bytes;
                // self.gap_start_bytes = pos_bytes;
                if del_len == 0 { return rm_start_bytes as usize; }
            }

            debug_assert!(del_len > 0);
            debug_assert!(pos >= self.gap_start_chars as usize);
        } else {
            // This is equivalent to self.count_bytes() (below), but for some reason manually
            // inlining it here results in both faster and smaller executables.
            let gap_bytes = if pos < gap_chars {
                self.int_str_get_byte_offset(self.start_as_str(), pos)
            } else {
                self.int_str_get_byte_offset(self.end_as_str(), pos - gap_chars) + self.gap_start_bytes as usize
            };
            self.move_gap(gap_bytes);
        }

        // At this point the gap is guaranteed to be directly after pos.
        let rm_end_bytes = self.int_str_get_byte_offset(self.end_as_str(), del_len);
        self.remove_at_gap(rm_end_bytes);
        rm_start_bytes as usize + rm_end_bytes
    }

    pub fn start_as_str(&self) -> &str {
        unsafe {
            slice_to_str(&self.data[0..self.gap_start_bytes as usize])
        }
    }
    pub fn end_as_str(&self) -> &str {
        unsafe {
            slice_to_str(&self.data[(self.gap_start_bytes +self.gap_len) as usize..LEN])
        }
    }

    pub fn count_bytes(&self, char_pos: usize) -> usize {
        let gap_chars = self.gap_start_chars as usize;
        let gap_bytes = self.gap_start_bytes as usize;
        // Clippy complains about this but if I swap to a match expression, performance drops by 1%.
        if char_pos == gap_chars {
            gap_bytes
        } else if char_pos < gap_chars {
            self.int_str_get_byte_offset(self.start_as_str(), char_pos)
        } else { // char_pos > start_char_len.
            gap_bytes + self.int_str_get_byte_offset(self.end_as_str(), char_pos - gap_chars)
        }
    }

    /// Take the remaining contents in the gap buffer. Mark them as deleted, but return them.
    /// This will leave those items non-zero, but that doesn't matter.
    pub fn take_rest(&mut self) -> &str {
        let last_idx = (self.gap_start_bytes +self.gap_len) as usize;
        self.gap_len = LEN as u16 - self.gap_start_bytes;
        unsafe { slice_to_str(&self.data[last_idx..LEN]) }
    }

    pub(crate) fn check(&self) {
        let char_len = count_chars(unsafe { slice_to_str(&self.data[..self.gap_start_bytes as usize]) });
        assert_eq!(char_len, self.gap_start_chars as usize);
    }
}

impl<const LEN: usize> ToString for GapBuffer<LEN> {
    fn to_string(&self) -> String {
        let mut result = String::with_capacity(self.len_bytes());
        result.push_str(self.start_as_str());
        result.push_str(self.end_as_str());
        result
    }
}

impl<const LEN: usize> PartialEq for GapBuffer<LEN> {
    // Eq is interesting because we need to ignore where the gap is.
    fn eq(&self, other: &Self) -> bool {
        if self.gap_len != other.gap_len { return false; }
        // There's 3 sections to check:
        // - Before our gap
        // - The inter-gap part
        // - The last, common part.
        let (a, b) = if self.gap_start_bytes < other.gap_start_bytes {
            (self, other)
        } else {
            (other, self)
        };
        // a has its gap first (or the gaps are at the same time).
        let a_start = a.gap_start_bytes as usize;
        let b_start = b.gap_start_bytes as usize;
        let gap_len = a.gap_len as usize;

        // Section before the gaps
        if a.data[0..a_start] != b.data[0..a_start] { return false; }

        // Gappy bit
        if a.data[a_start+gap_len..b_start+gap_len] != b.data[a_start..b_start] { return false; }

        // Last bit
        let end_idx = b_start + gap_len;
        a.data[end_idx..LEN] == b.data[end_idx..LEN]
    }
}

#[cfg(test)]
mod test {
    use crate::gapbuffer::GapBuffer;

    fn check_eq<const LEN: usize>(b: &GapBuffer<LEN>, s: &str) {
        assert_eq!(b.to_string(), s);
        assert_eq!(b.len_bytes(), s.len());
        assert_eq!(s.is_empty(), b.is_empty());
    }

    #[test]
    fn smoke_test() {
        let mut b = GapBuffer::<5>::new();

        b.try_insert(0, "hi").unwrap();
        b.try_insert(0, "x").unwrap(); // 'xhi'
        // b.move_gap(2);
        b.try_insert(2, "x").unwrap(); // 'xhxi'
        check_eq(&b, "xhxi");
    }

    #[test]
    fn remove() {
        let mut b = GapBuffer::<5>::new_from_str("hi");
        assert_eq!(b.remove(2, 2), 0);
        check_eq(&b, "hi");

        assert_eq!(b.remove(0, 1), 1);
        check_eq(&b, "i");

        assert_eq!(b.remove(0, 1000), 1);
        check_eq(&b, "");
    }

    #[test]
    fn eq() {
        let hi = GapBuffer::<5>::new_from_str("hi");
        let yo = GapBuffer::<5>::new_from_str("yo");
        assert_ne!(hi, yo);
        assert_eq!(hi, hi);

        let mut hi2 = GapBuffer::<5>::new_from_str("hi");
        hi2.move_gap(1);
        assert_eq!(hi, hi2);

        hi2.move_gap(0);
        assert_eq!(hi, hi2);
    }
}