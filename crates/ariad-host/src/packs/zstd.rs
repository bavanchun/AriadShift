//! Streaming zstd reader that joins data frames and skips metadata frames.

use std::io::{self, BufRead, Read};

use ruzstd::decoding::{
    BlockDecodingStrategy, FrameDecoder,
    errors::{FrameDecoderError, ReadFrameHeaderError},
};

/// `StreamingDecoder` reads only one frame; tar needs one continuous decoded stream.
pub(super) struct Decoder<R> {
    source: R,
    frame: FrameDecoder,
    initialized: bool,
}

impl<R> Decoder<R> {
    pub(super) fn new(source: R) -> Self {
        Self {
            source,
            frame: FrameDecoder::new(),
            initialized: false,
        }
    }
}

impl<R: BufRead> Read for Decoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if self.initialized {
                if self.frame.can_collect() > 0 {
                    return self.frame.read(buf);
                }
                if !self.frame.is_finished() {
                    self.frame
                        .decode_blocks(&mut self.source, BlockDecodingStrategy::UptoBlocks(1))
                        .map_err(io::Error::other)?;
                    continue;
                }
            }
            if self.source.fill_buf()?.is_empty() {
                return Ok(0);
            }
            self.initialized = false;
            match self.frame.init(&mut self.source) {
                Ok(()) => self.initialized = true,
                Err(FrameDecoderError::ReadFrameHeaderError(ReadFrameHeaderError::SkipFrame {
                    length,
                    ..
                })) => {
                    let skipped = io::copy(
                        &mut self.source.by_ref().take(u64::from(length)),
                        &mut io::sink(),
                    )?;
                    if skipped != u64::from(length) {
                        return Err(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "truncated zstd metadata frame",
                        ));
                    }
                }
                Err(error) => return Err(io::Error::other(error)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(bytes: &[u8]) -> Vec<u8> {
        ruzstd::encoding::compress_to_vec(bytes, ruzstd::encoding::CompressionLevel::Fastest)
    }

    fn metadata(bytes: &[u8]) -> Vec<u8> {
        let mut data = 0x184D_2A50_u32.to_le_bytes().to_vec();
        data.extend(u32::try_from(bytes.len()).unwrap().to_le_bytes());
        data.extend(bytes);
        data
    }

    #[test]
    fn frames_form_one_stream_including_empty_and_metadata_frames() {
        let data = [
            metadata(b"header"),
            frame(b"first"),
            frame(b""),
            metadata(b"index"),
            frame(b"second"),
            frame(b""),
        ]
        .concat();
        let mut reader = Decoder::new(io::Cursor::new(data));
        assert_eq!(reader.read(&mut []).unwrap(), 0);
        let mut first = [0_u8; 1];
        reader.read_exact(&mut first).unwrap();
        assert_eq!(&first, b"f");
        let mut rest = Vec::new();
        reader.read_to_end(&mut rest).unwrap();
        assert_eq!(rest, b"irstsecond");
    }

    #[test]
    fn truncated_data_and_metadata_frames_fail() {
        for data in [frame(b"content"), metadata(b"index")] {
            for end in 1..data.len() {
                let mut reader = Decoder::new(io::Cursor::new(&data[..end]));
                assert!(reader.read_to_end(&mut Vec::new()).is_err(), "end: {end}");
            }
        }
    }
}
