//! Open file handles for `fileOpen()` and the BIFs that work on them.
//!
//! A [`FileObject`] keeps a real operating-system handle, so reads move a
//! position forward the way the BoxLang JVM's `BoxFile` does. Text reads decode
//! with the charset given to `fileOpen()` and text writes encode with it.
//! The BIFs in [`super::fs`] accept either a file object or a path; for a path
//! they open a short-lived [`FileObject`] and close it again.

use crate::types::{BxNativeObject, BxVM, BxValue};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};

/// How many bytes a text reader pulls from the file at a time.
const READ_CHUNK: usize = 8 * 1024;

/// The line separator `writeLine()` puts before each new line, as Java's
/// `System.lineSeparator()` does.
#[cfg(windows)]
const LINE_SEPARATOR: &str = "\r\n";
/// The line separator `writeLine()` puts before each new line, as Java's
/// `System.lineSeparator()` does.
#[cfg(not(windows))]
const LINE_SEPARATOR: &str = "\n";

/// A text charset that `fileOpen()` can read and write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Charset {
    /// UTF-8, the default.
    Utf8,
    /// 7-bit US-ASCII.
    Ascii,
    /// ISO-8859-1, where each byte is the Unicode code point of the same value.
    Latin1,
    /// Windows-1252, decoded and encoded with `encoding_rs`.
    Windows1252,
}

impl Charset {
    /// Looks up a charset by name, ignoring case.
    ///
    /// Returns `None` for a name MatchBox does not support.
    pub(crate) fn from_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "utf-8" | "utf8" => Some(Self::Utf8),
            "us-ascii" | "ascii" => Some(Self::Ascii),
            "iso-8859-1" | "latin1" => Some(Self::Latin1),
            "windows-1252" => Some(Self::Windows1252),
            _ => None,
        }
    }

    /// Creates a streaming decoder for this charset.
    fn decoder(self) -> TextDecoder {
        let inner = match self {
            Self::Utf8 => Some(encoding_rs::UTF_8.new_decoder_without_bom_handling()),
            Self::Windows1252 => Some(encoding_rs::WINDOWS_1252.new_decoder_without_bom_handling()),
            Self::Ascii | Self::Latin1 => None,
        };
        TextDecoder {
            charset: self,
            inner,
        }
    }

    /// Encodes `text` in this charset.
    ///
    /// A character the charset cannot hold becomes `?`, as Java's encoders do.
    fn encode(self, text: &str) -> Vec<u8> {
        match self {
            Self::Utf8 => text.as_bytes().to_vec(),
            Self::Ascii => text
                .chars()
                .map(|character| {
                    if character.is_ascii() {
                        character as u8
                    } else {
                        b'?'
                    }
                })
                .collect(),
            Self::Latin1 => text
                .chars()
                .map(|character| u8::try_from(u32::from(character)).unwrap_or(b'?'))
                .collect(),
            Self::Windows1252 => {
                let mut bytes = Vec::with_capacity(text.len());
                let mut buffer = [0u8; 4];
                for character in text.chars() {
                    let (encoded, _, unmappable) =
                        encoding_rs::WINDOWS_1252.encode(character.encode_utf8(&mut buffer));
                    if unmappable {
                        bytes.push(b'?');
                    } else {
                        bytes.extend_from_slice(&encoded);
                    }
                }
                bytes
            }
        }
    }
}

/// Decodes bytes in one charset into text, across chunk boundaries.
struct TextDecoder {
    /// The charset being decoded.
    charset: Charset,
    /// The `encoding_rs` decoder for multi-byte or table-based charsets.
    inner: Option<encoding_rs::Decoder>,
}

impl TextDecoder {
    /// Decodes `bytes` and appends the text to `out`.
    ///
    /// Invalid input becomes U+FFFD, as Java's decoders do with `REPLACE`.
    /// `last` tells the decoder no more bytes follow, so a truncated sequence
    /// at the end is replaced too.
    fn decode(&mut self, bytes: &[u8], last: bool, out: &mut String) {
        match (&mut self.inner, self.charset) {
            (Some(decoder), _) => {
                let mut input = bytes;
                loop {
                    let needed = decoder
                        .max_utf8_buffer_length(input.len())
                        .unwrap_or(input.len() * 3 + 4);
                    out.reserve(needed);
                    let (result, read, _) = decoder.decode_to_string(input, out, last);
                    input = &input[read..];
                    if result == encoding_rs::CoderResult::InputEmpty {
                        break;
                    }
                }
            }
            (None, Charset::Latin1) => out.extend(bytes.iter().map(|&byte| char::from(byte))),
            (None, _) => out.extend(bytes.iter().map(|&byte| {
                if byte.is_ascii() {
                    char::from(byte)
                } else {
                    char::REPLACEMENT_CHARACTER
                }
            })),
        }
    }
}

/// Reads decoded text from a file, keeping a position like Java's `BufferedReader`.
struct TextReader {
    /// The open file.
    file: File,
    /// The decoder for the file's charset.
    decoder: TextDecoder,
    /// Decoded text that has not been consumed yet starts at `offset`.
    buffer: String,
    /// The byte offset in `buffer` of the next unread character.
    offset: usize,
    /// Whether the end of the file has been reached and fully decoded.
    done: bool,
}

impl TextReader {
    /// Wraps an open file in a reader for `charset`.
    fn new(file: File, charset: Charset) -> Self {
        Self {
            file,
            decoder: charset.decoder(),
            buffer: String::new(),
            offset: 0,
            done: false,
        }
    }

    /// Returns the decoded text that has not been read yet.
    fn pending(&self) -> &str {
        &self.buffer[self.offset..]
    }

    /// Decodes the next chunk of the file into the buffer.
    ///
    /// Returns `false` when the file has no more text.
    ///
    /// # Errors
    /// Returns an error when the file cannot be read.
    fn fill(&mut self) -> io::Result<bool> {
        if self.done {
            return Ok(false);
        }
        if self.offset > 0 {
            self.buffer.drain(..self.offset);
            self.offset = 0;
        }
        let before = self.buffer.len();
        let mut chunk = [0u8; READ_CHUNK];
        while self.buffer.len() == before && !self.done {
            let read = self.file.read(&mut chunk)?;
            self.done = read == 0;
            self.decoder
                .decode(&chunk[..read], self.done, &mut self.buffer);
        }
        Ok(self.buffer.len() > before)
    }

    /// Reads the next line without its terminator.
    ///
    /// A line ends at `\n`, `\r` or `\r\n`. Returns `None` at the end of the file.
    ///
    /// # Errors
    /// Returns an error when the file cannot be read.
    fn read_line(&mut self) -> io::Result<Option<String>> {
        loop {
            if let Some(index) = self.pending().find(['\n', '\r']) {
                let start = self.offset;
                let end = start + index;
                if self.buffer.as_bytes()[end] == b'\r'
                    && end + 1 == self.buffer.len()
                    && !self.done
                {
                    // A `\r` at the end of the buffer may be the start of `\r\n`.
                    let consumed = end - start;
                    self.fill()?;
                    let end = self.offset + consumed;
                    return Ok(Some(self.take_line(end)));
                }
                return Ok(Some(self.take_line(end)));
            }
            if !self.fill()? {
                if self.pending().is_empty() {
                    return Ok(None);
                }
                let line = self.pending().to_string();
                self.offset = self.buffer.len();
                return Ok(Some(line));
            }
        }
    }

    /// Returns the text up to the terminator at byte `end`, and consumes the terminator.
    fn take_line(&mut self, end: usize) -> String {
        let line = self.buffer[self.offset..end].to_string();
        let bytes = self.buffer.as_bytes();
        let terminator = if bytes.get(end) == Some(&b'\r') && bytes.get(end + 1) == Some(&b'\n') {
            2
        } else {
            usize::from(end < bytes.len())
        };
        self.offset = end + terminator;
        line
    }

    /// Reads up to `count` characters. Returns an empty string at the end of the file.
    ///
    /// # Errors
    /// Returns an error when the file cannot be read.
    fn read_chars(&mut self, count: usize) -> io::Result<String> {
        let mut text = String::new();
        let mut remaining = count;
        while remaining > 0 {
            if self.pending().is_empty() && !self.fill()? {
                break;
            }
            let taken: String = self.pending().chars().take(remaining).collect();
            remaining -= taken.chars().count();
            self.offset += taken.len();
            text.push_str(&taken);
        }
        Ok(text)
    }

    /// Reads all the text from the current position to the end of the file.
    ///
    /// # Errors
    /// Returns an error when the file cannot be read.
    fn read_all(&mut self) -> io::Result<String> {
        while self.fill()? {}
        let text = self.pending().to_string();
        self.offset = self.buffer.len();
        Ok(text)
    }

    /// Reports whether no text is left to read.
    ///
    /// # Errors
    /// Returns an error when the file cannot be read.
    fn is_eof(&mut self) -> io::Result<bool> {
        Ok(self.pending().is_empty() && !self.fill()?)
    }
}

/// The open mode of a [`FileObject`], named as `fileOpen()` takes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    /// Read decoded text.
    Read,
    /// Read raw bytes.
    ReadBinary,
    /// Write. An existing file is not emptied: without `seekable` writes go to
    /// its end, with `seekable` they start at the beginning, as in BoxLang.
    Write,
    /// Write to the end of the file.
    Append,
}

impl Mode {
    /// Looks up a mode by name, ignoring case.
    ///
    /// # Errors
    /// Returns an error naming the valid modes when `name` is not one of them.
    pub(crate) fn from_name(name: &str) -> Result<Self, String> {
        match name.to_ascii_lowercase().as_str() {
            "read" => Ok(Self::Read),
            "readbinary" => Ok(Self::ReadBinary),
            "write" => Ok(Self::Write),
            "append" => Ok(Self::Append),
            _ => Err(format!(
                "Invalid file mode: [{name}]. Valid modes are: read, readbinary, write, append"
            )),
        }
    }
}

/// The operating-system handle behind a [`FileObject`].
enum Handle {
    /// A text reader, for [`Mode::Read`].
    Text(TextReader),
    /// A byte reader and its file size, for [`Mode::ReadBinary`].
    Binary {
        /// The open file.
        file: File,
        /// The file size when it was opened.
        size: u64,
    },
    /// A writer, for [`Mode::Write`] and [`Mode::Append`].
    Writer(File),
    /// No handle, after `close()`.
    Closed,
}

/// The file object returned by `fileOpen()`.
pub(crate) struct FileObject {
    /// The absolute path of the file.
    path: String,
    /// The mode the file was opened in.
    mode: Mode,
    /// The charset for text reads and writes.
    charset: Charset,
    /// Whether `seek()` is allowed.
    seekable: bool,
    /// The open handle, or [`Handle::Closed`].
    handle: Handle,
}

impl fmt::Debug for FileObject {
    /// Shows the path, mode and whether the file is still open.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileObject")
            .field("path", &self.path)
            .field("mode", &self.mode)
            .field("open", &!matches!(self.handle, Handle::Closed))
            .finish()
    }
}

impl FileObject {
    /// Opens `path` in `mode`.
    ///
    /// `seekable` defaults to `true` for the read modes and `false` for the
    /// write modes. The write modes create the file when it is missing.
    ///
    /// # Errors
    /// Returns an error when the file cannot be opened, for example because a
    /// read mode was asked for and the file does not exist or is not readable.
    pub(crate) fn open(
        path: &str,
        mode: Mode,
        charset: Charset,
        seekable: Option<bool>,
    ) -> Result<Self, String> {
        let seekable = seekable.unwrap_or(matches!(mode, Mode::Read | Mode::ReadBinary));
        let unreadable = |_| format!("The file [{path}] does not exist or is not readable.");
        let handle = match mode {
            Mode::Read => Handle::Text(TextReader::new(
                File::open(path).map_err(unreadable)?,
                charset,
            )),
            Mode::ReadBinary => {
                let file = File::open(path).map_err(unreadable)?;
                let size = file.metadata().map_err(|error| error.to_string())?.len();
                Handle::Binary { file, size }
            }
            Mode::Write if seekable => Handle::Writer(
                OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(false)
                    .open(path)
                    .map_err(|error| error.to_string())?,
            ),
            Mode::Write | Mode::Append => Handle::Writer(
                OpenOptions::new()
                    .append(true)
                    .create(true)
                    .open(path)
                    .map_err(|error| error.to_string())?,
            ),
        };
        let absolute = std::path::absolute(path)
            .map(|absolute| absolute.to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string());
        Ok(Self {
            path: absolute,
            mode,
            charset,
            seekable,
            handle,
        })
    }

    /// Reads the next line, or returns `None` at the end of the file.
    ///
    /// # Errors
    /// Returns an error when the object is not open in `read` mode, or when the
    /// file cannot be read.
    pub(crate) fn read_line(&mut self) -> Result<Option<String>, String> {
        match &mut self.handle {
            Handle::Text(reader) => reader.read_line().map_err(|error| error.to_string()),
            _ => Err("This file object was not opened in read mode".to_string()),
        }
    }

    /// Appends `content` as a new line.
    ///
    /// When the file is not empty a line separator is written first, so the
    /// file never ends with one, as in BoxLang.
    ///
    /// # Errors
    /// Returns an error when the object is not open in a write mode, or when
    /// the write fails.
    pub(crate) fn write_line(&mut self, content: &str) -> Result<(), String> {
        let is_empty = fs::metadata(&self.path)
            .map(|metadata| metadata.len() == 0)
            .unwrap_or(true);
        let text = if is_empty {
            content.to_string()
        } else {
            format!("{LINE_SEPARATOR}{content}")
        };
        self.write_bytes(&self.charset.encode(&text))
    }

    /// Writes raw bytes at the current position.
    ///
    /// # Errors
    /// Returns an error when the object is not open in a write mode, or when
    /// the write fails.
    fn write_bytes(&mut self, bytes: &[u8]) -> Result<(), String> {
        match &mut self.handle {
            Handle::Writer(file) => file.write_all(bytes).map_err(|error| error.to_string()),
            _ => Err("This file object is not writeable. Operation disallowed.".to_string()),
        }
    }

    /// Writes a value: bytes as they are, anything else as text in the charset.
    ///
    /// # Errors
    /// Returns an error when the object is not open in a write mode, when a
    /// binary value cannot be read, or when the write fails.
    fn write_value(&mut self, vm: &mut dyn BxVM, value: BxValue) -> Result<(), String> {
        if vm.is_bytes(value) {
            let bytes = vm.to_bytes(value)?;
            self.write_bytes(&bytes)
        } else {
            let text = vm.to_string(value);
            self.write_bytes(&self.charset.encode(&text))
        }
    }

    /// Reads up to `count` characters (text) or bytes (binary) into a value.
    ///
    /// Returns an empty string or empty binary value at the end of the file.
    ///
    /// # Errors
    /// Returns an error when the object is not open in a read mode, or when the
    /// file cannot be read.
    fn read_count(&mut self, vm: &mut dyn BxVM, count: usize) -> Result<BxValue, String> {
        match &mut self.handle {
            Handle::Text(reader) => {
                let text = reader
                    .read_chars(count)
                    .map_err(|error| error.to_string())?;
                Ok(BxValue::new_ptr(vm.string_new(text)))
            }
            Handle::Binary { file, .. } => {
                let mut bytes = Vec::with_capacity(count);
                Read::by_ref(file)
                    .take(count as u64)
                    .read_to_end(&mut bytes)
                    .map_err(|error| error.to_string())?;
                Ok(BxValue::new_ptr(vm.bytes_new(bytes)))
            }
            _ => Err("This file object was not opened in read mode".to_string()),
        }
    }

    /// Reads everything from the current position to the end of the file.
    ///
    /// Returns a string in `read` mode and a binary value in `readBinary` mode.
    ///
    /// # Errors
    /// Returns an error when the object is not open in a read mode, or when the
    /// file cannot be read.
    fn read_all(&mut self, vm: &mut dyn BxVM) -> Result<BxValue, String> {
        match &mut self.handle {
            Handle::Text(reader) => {
                let text = reader.read_all().map_err(|error| error.to_string())?;
                Ok(BxValue::new_ptr(vm.string_new(text)))
            }
            Handle::Binary { file, .. } => {
                let mut bytes = Vec::new();
                file.read_to_end(&mut bytes)
                    .map_err(|error| error.to_string())?;
                Ok(BxValue::new_ptr(vm.bytes_new(bytes)))
            }
            _ => Err(format!(
                "Cannot read from file [{}] — it is not opened in a read mode.",
                self.path
            )),
        }
    }

    /// Moves the position, as BoxLang's `seek()` does.
    ///
    /// In `read` mode it skips `offset` characters forward from the current
    /// position. In the other modes it moves to byte `offset` from the start.
    ///
    /// # Errors
    /// Returns an error when the object was not opened as seekable, when it is
    /// closed, or when the file cannot be read or positioned.
    fn seek(&mut self, offset: u64) -> Result<(), String> {
        if !self.seekable {
            return Err(format!(
                "This file instance was opened with the seekable argument set to false. This operation for file [{}] is not allowed",
                self.path
            ));
        }
        match &mut self.handle {
            Handle::Text(reader) => {
                let count = usize::try_from(offset).map_err(|error| error.to_string())?;
                reader
                    .read_chars(count)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            }
            Handle::Binary { file, .. } | Handle::Writer(file) => file
                .seek(SeekFrom::Start(offset))
                .map(|_| ())
                .map_err(|error| error.to_string()),
            Handle::Closed => Err("This file object was not opened in read mode".to_string()),
        }
    }

    /// Reports whether the position is at the end of the file.
    ///
    /// # Errors
    /// Returns an error when the object is open in a write mode or closed, or
    /// when the file cannot be read.
    fn is_eof(&mut self) -> Result<bool, String> {
        match &mut self.handle {
            Handle::Text(reader) => reader.is_eof().map_err(|error| error.to_string()),
            Handle::Binary { file, size } => file
                .stream_position()
                .map(|position| position >= *size)
                .map_err(|error| error.to_string()),
            _ => Err(
                "This file object is in write or append mode.  Unable to determine EOF."
                    .to_string(),
            ),
        }
    }

    /// Releases the handle. Later reads and writes fail.
    pub(crate) fn close(&mut self) {
        self.handle = Handle::Closed;
    }
}

/// Reads a whole-number argument such as a count or position.
///
/// # Errors
/// Returns an error when the value is missing or negative.
fn count_argument(vm: &mut dyn BxVM, value: Option<&BxValue>, name: &str) -> Result<u64, String> {
    let value = value.ok_or_else(|| format!("The [{name}] argument is required"))?;
    let number = if value.is_int() {
        value.as_int() as f64
    } else if value.is_number() {
        value.as_number()
    } else {
        vm.to_string(*value)
            .trim()
            .parse::<f64>()
            .map_err(|_| format!("The [{name}] argument must be a number"))?
    };
    if number < 0.0 {
        return Err(format!("The [{name}] argument must not be negative"));
    }
    Ok(number as u64)
}

impl BxNativeObject for FileObject {
    /// Returns null, because file objects expose their data through methods.
    fn get_property(&self, _name: &str) -> BxValue {
        BxValue::new_null()
    }

    /// Ignores the assignment, because file properties are read-only.
    fn set_property(&mut self, _name: &str, _value: BxValue) {}

    /// Runs a file object method, matched case-insensitively.
    ///
    /// - `__is_file` returns `true`, so BIFs can tell file objects from paths.
    /// - `toString` returns the path.
    /// - `readLine` returns the next line, or null at the end of the file.
    /// - `read( count )` reads up to `count` characters (text) or bytes (binary).
    /// - `readAll` reads the rest of the file.
    /// - `write( data )` and `append( data )` write at the current position.
    /// - `writeLine( data )` writes `data` as a new line (see [`FileObject::write_line`]).
    /// - `seek( offset )` and `skipBytes( offset )` move the position (see [`FileObject::seek`]).
    /// - `isEOF` reports whether the position is at the end of the file.
    /// - `close` releases the handle.
    ///
    /// Methods that change the file return the object, so calls can chain.
    ///
    /// # Errors
    /// Returns an error for an unknown method, a missing argument, a method
    /// that does not fit the open mode, or a failed read or write.
    fn call_method(
        &mut self,
        vm: &mut dyn BxVM,
        id: usize,
        name: &str,
        args: &[BxValue],
    ) -> Result<BxValue, String> {
        let string = |vm: &mut dyn BxVM, text: String| BxValue::new_ptr(vm.string_new(text));
        match name.to_ascii_lowercase().as_str() {
            "__is_file" => Ok(BxValue::new_bool(true)),
            "tostring" => Ok(string(vm, self.path.clone())),
            "readline" => match self.read_line()? {
                Some(line) => Ok(string(vm, line)),
                None => Ok(BxValue::new_null()),
            },
            "read" => match args.first() {
                Some(_) => {
                    let count = count_argument(vm, args.first(), "len")?;
                    self.read_count(
                        vm,
                        usize::try_from(count).map_err(|error| error.to_string())?,
                    )
                }
                None => self.read_all(vm),
            },
            "readall" => self.read_all(vm),
            "write" | "append" => {
                let data = *args
                    .first()
                    .ok_or_else(|| format!("{name}() expects 1 argument"))?;
                self.write_value(vm, data)?;
                Ok(BxValue::new_ptr(id))
            }
            "writeline" => {
                let data = args
                    .first()
                    .map(|value| vm.to_string(*value))
                    .ok_or_else(|| "writeLine() expects 1 argument".to_string())?;
                self.write_line(&data)?;
                Ok(BxValue::new_ptr(id))
            }
            "seek" | "skipbytes" => {
                let offset = count_argument(vm, args.first(), "position")?;
                self.seek(offset)?;
                Ok(BxValue::new_ptr(id))
            }
            "iseof" => Ok(BxValue::new_bool(self.is_eof()?)),
            "close" => {
                self.close();
                Ok(BxValue::new_null())
            }
            _ => Err(format!("File method '{}' not found", name)),
        }
    }
}

/// Returns the heap id of `value` when it is a file object from `fileOpen()`.
pub(crate) fn file_object_id(vm: &mut dyn BxVM, value: BxValue) -> Option<usize> {
    let id = value.as_gc_id()?;
    match vm.native_object_call_method(id, "__is_file", &[]) {
        Ok(result) if result.is_bool() && result.as_bool() => Some(id),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Charset, Mode, TextReader};
    use std::io::Write;

    /// A temporary file that is removed when dropped.
    struct TempFile(std::path::PathBuf);

    impl Drop for TempFile {
        /// Removes the file, ignoring errors.
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// Writes `bytes` to a new temporary file and opens a text reader on it.
    fn reader_for(bytes: &[u8], charset: Charset) -> (TempFile, TextReader) {
        let path = std::env::temp_dir().join(format!(
            "matchbox-file-handle-{}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::File::create(&path)
            .and_then(|mut file| file.write_all(bytes))
            .expect("write temp file");
        let reader = TextReader::new(std::fs::File::open(&path).expect("open temp file"), charset);
        (TempFile(path), reader)
    }

    #[test]
    fn reads_lines_with_every_terminator() {
        let (_file, mut reader) = reader_for(b"a\nb\r\nc\rd", Charset::Utf8);
        let lines: Vec<_> = std::iter::from_fn(|| reader.read_line().unwrap()).collect();
        assert_eq!(lines, ["a", "b", "c", "d"]);
        assert!(reader.is_eof().unwrap());
    }

    #[test]
    fn keeps_empty_lines_and_has_no_trailing_line() {
        let (_file, mut reader) = reader_for(b"a\n\nb\n", Charset::Utf8);
        let lines: Vec<_> = std::iter::from_fn(|| reader.read_line().unwrap()).collect();
        assert_eq!(lines, ["a", "", "b"]);
    }

    #[test]
    fn joins_crlf_across_chunk_boundary() {
        let mut bytes = vec![b'x'; super::READ_CHUNK - 1];
        bytes.extend_from_slice(b"\r\nnext");
        let (_file, mut reader) = reader_for(&bytes, Charset::Utf8);
        assert_eq!(
            reader.read_line().unwrap().unwrap().len(),
            super::READ_CHUNK - 1
        );
        assert_eq!(reader.read_line().unwrap().as_deref(), Some("next"));
        assert_eq!(reader.read_line().unwrap(), None);
    }

    #[test]
    fn decodes_each_charset() {
        let (_file, mut latin1) = reader_for(&[b'c', 0xE9, 0x80], Charset::Latin1);
        assert_eq!(latin1.read_all().unwrap(), "c\u{e9}\u{80}");
        let (_file, mut cp1252) = reader_for(&[b'c', 0xE9, 0x80], Charset::Windows1252);
        assert_eq!(cp1252.read_all().unwrap(), "c\u{e9}\u{20ac}");
        let (_file, mut ascii) = reader_for(&[b'c', 0xE9], Charset::Ascii);
        assert_eq!(ascii.read_all().unwrap(), "c\u{fffd}");
        let (_file, mut utf8) = reader_for("c\u{e9}".as_bytes(), Charset::Utf8);
        assert_eq!(utf8.read_all().unwrap(), "c\u{e9}");
    }

    #[test]
    fn encodes_each_charset_with_question_mark_for_unmappable() {
        assert_eq!(Charset::Latin1.encode("\u{e9}\u{20ac}"), [0xE9, b'?']);
        assert_eq!(
            Charset::Windows1252.encode("\u{e9}\u{20ac}\u{4e2d}"),
            [0xE9, 0x80, b'?']
        );
        assert_eq!(Charset::Ascii.encode("a\u{e9}"), [b'a', b'?']);
        assert_eq!(Charset::Utf8.encode("\u{e9}"), "\u{e9}".as_bytes());
    }

    #[test]
    fn reads_characters_not_bytes() {
        let (_file, mut reader) = reader_for("\u{e9}t\u{e9}".as_bytes(), Charset::Utf8);
        assert_eq!(reader.read_chars(2).unwrap(), "\u{e9}t");
        assert_eq!(reader.read_chars(5).unwrap(), "\u{e9}");
        assert_eq!(reader.read_chars(1).unwrap(), "");
    }

    #[test]
    fn rejects_unknown_names() {
        assert!(Charset::from_name("blah").is_none());
        assert!(Mode::from_name("blah").is_err());
        assert_eq!(Charset::from_name(" UTF8 "), Some(Charset::Utf8));
    }
}
