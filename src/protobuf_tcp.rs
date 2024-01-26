//! Protobuf over TCP.

use std::io;
use thiserror::Error;
use tokio::io::{split, AsyncReadExt, AsyncWriteExt, BufReader, BufWriter, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio_rustls::TlsStream;

/// Protobuf over TCP error.
#[allow(missing_docs)]
#[derive(Error, Debug)]
pub enum Error {
    #[error("I/O: {0}")]
    Io(#[from] io::Error),
    #[error("Protobuf decode: {0}")]
    Decode(#[from] prost::DecodeError),
    #[error("Protobuf encode: {0}")]
    Encode(#[from] prost::EncodeError),
    #[error("The length preceding the protobuf value is not valid")]
    InvalidLen,
}

/// Protobuf over TCP reader.
pub struct Reader {
    reader: BufReader<ReadHalf<TlsStream<TcpStream>>>,
    buffer: Vec<u8>,
    max_len: usize,
}

/// Protobuf over TCP writer.
pub struct Writer {
    writer: BufWriter<WriteHalf<TlsStream<TcpStream>>>,
    buffer: Vec<u8>,
    max_len: usize,
}

/// Creates a new pair of [`Reader`] and [`Writer`].
pub fn new(sock: TlsStream<TcpStream>, max_len: usize) -> (Reader, Writer) {
    let (reader, writer) = split(sock);
    let reader = Reader {
        reader: BufReader::new(reader),
        buffer: Vec::new(),
        max_len,
    };
    let writer = Writer {
        writer: BufWriter::new(writer),
        buffer: Vec::new(),
        max_len,
    };
    (reader, writer)
}

impl Reader {
    /// Reads and decodes the next message from the socket.
    pub async fn read<T: prost::Message + Default>(&mut self) -> Result<T, Error> {
        let length = self.reader.read_u32().await? as usize;
        if length > self.max_len {
            return Err(Error::InvalidLen);
        }
        self.buffer.clear();
        self.buffer.resize(length, 0);
        self.reader.read_exact(&mut self.buffer).await?;
        Ok(T::decode(self.buffer.as_slice())?)
    }
}

impl Writer {
    /// Encodes and sends a message over the socket.
    pub async fn write<T: prost::Message>(&mut self, message: T) -> Result<(), Error> {
        let length = message.encoded_len();
        if length > self.max_len {
            return Err(Error::InvalidLen);
        }
        self.writer.write_u32(length.try_into().unwrap()).await?;
        self.buffer.clear();
        message.encode(&mut self.buffer)?;
        self.writer.write_all(&self.buffer).await?;
        Ok(())
    }

    /// Flushes the socket.
    pub async fn flush(&mut self) -> Result<(), Error> {
        self.writer.flush().await?;
        Ok(())
    }
}
