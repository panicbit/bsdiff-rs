use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};
use std::mem::size_of;

use anyhow::{bail, ensure, Context, Result};
use byteorder::{ReadBytesExt, LE};
use bzip2::read::BzDecoder;
use itertools::izip;

const MAGIC: &[u8] = b"BSDIFF40";

pub struct Bsdiff4 {
    new_size: usize,
    control: Vec<Control>,
    diff: Vec<u8>,
    extra: Vec<u8>,
}

impl Bsdiff4 {
    pub fn read<R: Read>(r: &mut R) -> Result<Self> {
        {
            let mut magic = [0; MAGIC.len()];

            r.read_exact(&mut magic)
                .context("failed to read bsdiff4 magic")?;

            ensure!(magic == MAGIC, "invalid bsdiff4 magic");
        }

        let len_control = read_positive_le_i64(r).context("failed to read `len_control`")?;
        let len_diff = read_positive_le_i64(r).context("failed to read `len_diff`")?;
        let new_size = read_positive_le_i64(r).context("failed to read `new_size`")?;
        let new_size = usize::try_from(new_size)?;

        dbg!(len_control, len_diff, new_size);

        let control = read_control_block(r, len_control).context("failed to read `bcontrol`")?;
        let diff = read_bzip_block(r, len_diff)?;
        let extra = read_bzip_to_end(r)?;

        eprintln!("{:#?}", control.iter().take(10).collect::<Vec<_>>());

        Ok(Bsdiff4 {
            new_size,
            control,
            diff,
            extra,
        })
    }

    pub fn apply(&self, original: &mut (impl Read + Seek), new: &mut impl Write) -> Result<()> {
        let new_chunk = &mut Vec::new();
        let diff_chunk = &mut Vec::new();
        let diff = &mut Cursor::new(&self.diff);
        let extra = &mut Cursor::new(&self.extra);

        for control in &self.control {
            new_chunk.clear();
            diff_chunk.clear();

            copy_exact(original, new_chunk, control.diff_amount)?;
            copy_exact(diff, diff_chunk, control.diff_amount)?;

            for (orig, diff) in izip!(new_chunk.iter_mut(), diff_chunk.iter()) {
                *orig = orig.wrapping_add(*diff);
            }

            io::copy(&mut new_chunk.as_slice(), new)?;

            copy_exact(extra, new, control.extra_amount)?;

            original.seek(SeekFrom::Current(control.seek))?;
        }

        Ok(())
    }

    pub fn apply_to_slice(&self, original: &[u8]) -> Result<Vec<u8>> {
        let mut new = Vec::with_capacity(self.new_size);

        self.apply(&mut Cursor::new(original), &mut new)?;

        Ok(new)
    }
}

fn read_control_block(r: &mut impl Read, len: u64) -> Result<Vec<Control>> {
    const CONTROL_SIZE: usize = 3 * size_of::<u64>();
    let block = read_bzip_block(r, len)?;

    if (block.len() % CONTROL_SIZE) != 0 {
        bail!("invalid control block size");
    }

    let num_control = block.len() / CONTROL_SIZE;
    let mut control_block = Vec::with_capacity(num_control);

    let mut block = Cursor::new(block);

    for _ in 0..num_control {
        let control = Control {
            diff_amount: read_positive_le_i64(&mut block)?,
            extra_amount: read_positive_le_i64(&mut block)?,
            seek: read_ones_complement_le_i64(&mut block)?,
        };

        control_block.push(control)
    }

    Ok(control_block)
}

#[derive(Debug)]
struct Control {
    diff_amount: u64,
    extra_amount: u64,
    seek: i64,
}

fn read_bzip_block(r: &mut impl Read, len: u64) -> Result<Vec<u8>> {
    let mut block = Vec::with_capacity(len as usize);
    BzDecoder::new(r.take(len)).read_to_end(&mut block)?;

    Ok(block)
}

fn read_bzip_to_end(r: &mut impl Read) -> Result<Vec<u8>> {
    let mut block = Vec::new();
    BzDecoder::new(r).read_to_end(&mut block)?;

    Ok(block)
}

fn read_ones_complement_le_i64(r: &mut impl Read) -> Result<i64> {
    let n = r.read_i64::<LE>()?;
    let n = ones_complement_i64(n);

    Ok(n)
}

fn read_positive_le_i64(r: &mut impl Read) -> Result<u64> {
    let n = r.read_i64::<LE>()?;
    let n = u64::try_from(n)?;

    Ok(n)
}

const fn ones_complement_i64(y: i64) -> i64 {
    y & i64::MIN | y.wrapping_abs()
}

pub fn copy_exact<R, W>(reader: &mut R, writer: &mut W, amount: u64) -> Result<()>
where
    R: Read + ?Sized,
    W: Write + ?Sized,
{
    struct Meter<W> {
        writer: W,
        bytes_written: usize,
    }

    impl<W> Write for Meter<W>
    where
        W: Write,
    {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let bytes_written = self.writer.write(buf)?;

            self.bytes_written += bytes_written;

            Ok(bytes_written)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.writer.flush()
        }
    }

    let mut reader = reader.take(amount);
    let mut writer = Meter {
        writer,
        bytes_written: 0,
    };

    io::copy(&mut reader, &mut writer)?;

    let bytes_written = u64::try_from(writer.bytes_written)?;

    ensure!(bytes_written == amount, "copied less bytes than expected");

    Ok(())
}
