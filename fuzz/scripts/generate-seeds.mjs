#!/usr/bin/env node
// Genera un corpus mínimo para cada target de `fuzz/fuzz_targets`.
//
// Los archivos son binarios y se escriben en `fuzz/corpus/<target>/`. El
// workflow de fuzzing los regenera antes de cada ejecución; también se pueden
// crear en local con `node fuzz/scripts/generate-seeds.mjs`.
//
// No se usan claves ni contenido reales: todo son estructuras sintéticas.

import { createCipheriv } from "node:crypto";
import { mkdirSync, rmSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const corpusRoot = join(here, "..", "corpus");

const EMPTY = 0xffff_ffff;
const NCA3_HEADER_LEN = 0xc00;

function writeSeed(target, name, bytes) {
  const dir = join(corpusRoot, target);
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, name), bytes);
}

function u32le(value) {
  const out = Buffer.alloc(4);
  out.writeUInt32LE(value);
  return out;
}

function pfs0Seed() {
  const name = Buffer.from("a\0", "utf8");
  const header = Buffer.alloc(0x10);
  header.write("PFS0", 0, "ascii");
  header.writeUInt32LE(1, 0x04);
  header.writeUInt32LE(name.length, 0x08);

  const entry = Buffer.alloc(0x18);
  entry.writeBigUInt64LE(0n, 0x00);
  entry.writeBigUInt64LE(0n, 0x08);
  entry.writeUInt32LE(0, 0x10);

  return Buffer.concat([header, entry, name]);
}

function hfs0Seed() {
  const name = Buffer.from("a\0", "utf8");
  const header = Buffer.alloc(0x10);
  header.write("HFS0", 0, "ascii");
  header.writeUInt32LE(1, 0x04);
  header.writeUInt32LE(name.length, 0x08);

  const entry = Buffer.alloc(0x40);
  entry.writeBigUInt64LE(0n, 0x00);
  entry.writeBigUInt64LE(0n, 0x08);
  entry.writeUInt32LE(0, 0x10);
  entry.writeUInt32LE(0, 0x14);
  entry.fill(0xab, 0x20, 0x40);

  return Buffer.concat([header, entry, name]);
}

function xciSeed() {
  const root = Buffer.alloc(0x10);
  root.write("HFS0", 0, "ascii");
  root.writeUInt32LE(0, 0x04);
  root.writeUInt32LE(0, 0x08);

  const header = Buffer.alloc(0x200);
  header.write("HEAD", 0x100, "ascii");
  header.writeUInt32LE(0xf000, 0x104);
  header.writeBigUInt64LE(BigInt(header.length + root.length), 0x118);
  header.writeBigUInt64LE(BigInt(header.length), 0x130);
  header.writeBigUInt64LE(BigInt(root.length), 0x138);
  header.fill(0xcd, 0x140, 0x160);

  return Buffer.concat([header, root]);
}

function ncaPlaintextSeed() {
  const header = Buffer.alloc(NCA3_HEADER_LEN);
  header.write("NCA3", 0x200, "ascii");
  header.writeUInt8(0, 0x204); // distribution
  header.writeUInt8(1, 0x205); // content type: Meta
  header.writeUInt8(10, 0x206); // crypto type
  header.writeUInt8(0, 0x207); // KAEK index
  header.writeBigUInt64LE(BigInt(NCA3_HEADER_LEN), 0x208);
  header.writeBigUInt64LE(0x01006bb00c6f0000n, 0x210);
  header.writeUInt32LE(0x000c0000, 0x21c);
  header.writeUInt8(10, 0x220);

  // Sección 0: 0x4000..0x6000, CTR, RomFS.
  header.writeUInt32LE(0x20, 0x240);
  header.writeUInt32LE(0x30, 0x244);
  header.writeUInt8(0, 0x400 + 0x02); // partition type: RomFS
  header.writeUInt8(3, 0x400 + 0x03); // FS type: RomFS
  header.writeUInt8(3, 0x400 + 0x04); // encryption: CTR
  header.writeUInt32LE(2, 0x400 + 0x140);
  header.writeUInt32LE(1, 0x400 + 0x144);

  return header;
}

// AES-128-XTS con sectores de 0x200, igual que `crypto::xts_process`.
function mulAlpha(tweak) {
  let carry = 0;
  for (let i = 0; i < 16; i += 1) {
    const next = tweak[i] >> 7;
    tweak[i] = ((tweak[i] << 1) | carry) & 0xff;
    carry = next;
  }
  if (carry !== 0) {
    tweak[0] ^= 0x87;
  }
}

function xtsEncrypt(key, plain, sectorSize = 0x200) {
  const dataCipher = createCipheriv("aes-128-ecb", key.subarray(0, 16), null);
  dataCipher.setAutoPadding(false);
  const tweakCipher = createCipheriv("aes-128-ecb", key.subarray(16, 32), null);
  tweakCipher.setAutoPadding(false);

  const out = Buffer.from(plain);
  for (let sector = 0; sector < out.length / sectorSize; sector += 1) {
    const counter = Buffer.alloc(16);
    counter.writeBigUInt64BE(BigInt(sector), 8);
    const tweak = Buffer.from(tweakCipher.update(counter));

    const start = sector * sectorSize;
    const end = Math.min(start + sectorSize, out.length);
    for (let at = start; at + 16 <= end; at += 16) {
      const block = Buffer.alloc(16);
      for (let i = 0; i < 16; i += 1) {
        block[i] = out[at + i] ^ tweak[i];
      }
      const encrypted = dataCipher.update(block);
      for (let i = 0; i < 16; i += 1) {
        out[at + i] = encrypted[i] ^ tweak[i];
      }
      mulAlpha(tweak);
    }
  }
  return out;
}

function cnmtSeed() {
  const contentInfoLen = 0x38;
  const data = Buffer.alloc(0x20 + contentInfoLen);
  data.writeBigUInt64LE(0x0100000000010000n, 0x00);
  data.writeUInt32LE(0, 0x08);
  data.writeUInt8(0x80, 0x0c); // Application
  data.writeUInt16LE(0, 0x0e); // extended header
  data.writeUInt16LE(1, 0x10); // one content
  data.writeUInt16LE(0, 0x12); // no content meta
  data.writeUInt8(0, 0x14);
  data.writeUInt32LE(0, 0x18);

  const at = 0x20;
  data.fill(0xab, at, at + 0x20);
  data.fill(0xcd, at + 0x20, at + 0x30);
  data.writeUInt32LE(0x1000, at + 0x30);
  data.writeUInt16LE(0, at + 0x34);
  data.writeUInt8(0, at + 0x36);
  data.writeUInt8(0, at + 0x37);
  return data;
}

function ticketSeed() {
  const signatureType = 0x010006;
  const sigLen = 0x14;
  const pad = 0x28;
  const bodyLen = 0x180;

  const head = Buffer.alloc(4 + sigLen + pad);
  head.writeUInt32LE(signatureType, 0);
  head.fill(0x11, 4, 4 + sigLen);

  const body = Buffer.alloc(bodyLen);
  body.write("Root-CA00000003-XS00000020", 0, "ascii");
  body.fill(0x22, 0x40, 0x50); // encrypted titlekey
  body.writeUInt8(0, 0x141); // title key type: common
  body.writeUInt8(0, 0x145); // master key revision
  body.fill(0x33, 0x160, 0x170); // rights id

  return Buffer.concat([head, body]);
}

function romfsSeed() {
  const dir = Buffer.alloc(0x18);
  dir.writeUInt32LE(EMPTY, 0x00); // sibling
  dir.writeUInt32LE(EMPTY, 0x04);
  dir.writeUInt32LE(EMPTY, 0x08); // child
  dir.writeUInt32LE(0, 0x0c); // first file -> entry 0
  dir.writeUInt32LE(0, 0x14); // root name is empty

  const name = Buffer.from("a", "utf8");
  const file = Buffer.alloc(0x20 + name.length);
  file.writeUInt32LE(EMPTY, 0x04); // sibling
  file.writeBigUInt64LE(0n, 0x08);
  file.writeBigUInt64LE(0n, 0x10);
  file.writeUInt32LE(name.length, 0x1c);
  name.copy(file, 0x20);

  const dirMetaOffset = 0x50;
  const fileMetaOffset = dirMetaOffset + dir.length;
  const dataOffset = fileMetaOffset + file.length;

  const header = Buffer.alloc(0x50);
  header.writeBigUInt64LE(0x50n, 0x00); // header size
  header.writeBigUInt64LE(BigInt(dirMetaOffset), 0x18);
  header.writeBigUInt64LE(BigInt(dir.length), 0x20);
  header.writeBigUInt64LE(BigInt(fileMetaOffset), 0x38);
  header.writeBigUInt64LE(BigInt(file.length), 0x40);
  header.writeBigUInt64LE(BigInt(dataOffset), 0x48);

  return Buffer.concat([header, dir, file]);
}

function romfsHeaderSeed() {
  const header = Buffer.alloc(0x50);
  header.writeBigUInt64LE(0x50n, 0x00);
  header.writeBigUInt64LE(0x50n, 0x18);
  header.writeBigUInt64LE(0n, 0x20);
  header.writeBigUInt64LE(0x50n, 0x38);
  header.writeBigUInt64LE(0n, 0x40);
  header.writeBigUInt64LE(0x50n, 0x48);
  return header;
}

function ivfcSeed() {
  const header = Buffer.alloc(0xe0);
  header.write("IVFC", 0, "ascii");
  header.writeUInt32LE(1, 0x04);
  header.writeUInt32LE(0x20, 0x08);
  header.writeUInt32LE(5, 0x0c);

  for (let level = 0; level < 6; level += 1) {
    const at = 0x10 + level * 0x18;
    header.writeBigUInt64LE(BigInt(level * 0x1000), at);
    header.writeBigUInt64LE(0x1000n, at + 0x08);
    header.writeUInt32LE(12, at + 0x10);
  }
  header.fill(0xab, 0xc0, 0xe0);
  return header;
}

function bktrHeaderSeed() {
  const header = Buffer.alloc(0x20);
  header.writeBigUInt64LE(0n, 0x00);
  header.writeBigUInt64LE(0x8000n, 0x08);
  header.write("BKTR", 0x10, "ascii");
  header.writeUInt32LE(1, 0x14);
  header.writeUInt32LE(1, 0x18);
  return header;
}

function bktrTablesSeed() {
  const tableSize = 0x8000;
  const bucketAt = 0x4000;

  const reloc = Buffer.alloc(tableSize);
  reloc.writeUInt32LE(1, 0x04);
  reloc.writeBigUInt64LE(0x1000n, 0x08);
  reloc.writeUInt32LE(1, bucketAt + 0x04);
  reloc.writeBigUInt64LE(0n, bucketAt + 0x10); // virt
  reloc.writeBigUInt64LE(0n, bucketAt + 0x18); // phys
  reloc.writeUInt32LE(0, bucketAt + 0x20); // is_patch

  const subsec = Buffer.alloc(tableSize);
  subsec.writeUInt32LE(1, 0x04);
  subsec.writeBigUInt64LE(0x1000n, 0x08);
  subsec.writeUInt32LE(1, bucketAt + 0x04);
  subsec.writeBigUInt64LE(0n, bucketAt + 0x10); // phys
  subsec.writeUInt32LE(0, bucketAt + 0x1c); // ctr_val

  const framing = u32le(tableSize);
  return Buffer.concat([framing, reloc, subsec]);
}

function keysSeed() {
  return Buffer.from(
    [
      "# synthetic keyset for fuzzing",
      "header_key = 000102030405060708090A0B0C0D0E0F101112131415161718191A1B1C1D1E1F",
      "titlekek_00 = 000102030405060708090A0B0C0D0E0F",
      "key_area_key_application_00 = 000102030405060708090A0B0C0D0E0F",
      "01000000000000000000000000000000 = 000102030405060708090A0B0C0D0E0F",
      "0x0A = 00FF",
      "",
    ].join("\n"),
    "utf8",
  );
}

function sha256sumsSeed() {
  return Buffer.from(
    [
      `${"a".repeat(64)}  file.bin`,
      `${"b".repeat(64)} *dir/other.bin`,
      "",
    ].join("\n"),
    "utf8",
  );
}

rmSync(corpusRoot, { recursive: true, force: true });

writeSeed("containers", "pfs0.bin", pfs0Seed());
writeSeed("containers", "hfs0.bin", hfs0Seed());
writeSeed("containers", "xci.bin", xciSeed());

const ncaPlain = ncaPlaintextSeed();
writeSeed("nca", "plaintext.nca", ncaPlain);
writeSeed("nca", "encrypted.nca", xtsEncrypt(Buffer.alloc(32, 0x5a), ncaPlain));

writeSeed("metadata", "cnmt.bin", cnmtSeed());
writeSeed("metadata", "ticket.bin", ticketSeed());
writeSeed("metadata", "romfs-header.bin", romfsHeaderSeed());
writeSeed("metadata", "ivfc.bin", ivfcSeed());
writeSeed("metadata", "bktr-header.bin", bktrHeaderSeed());

writeSeed("romfs", "simple.romfs", romfsSeed());
writeSeed("bktr", "tables.bktr", bktrTablesSeed());
writeSeed("keys", "prod.keys.txt", keysSeed());
writeSeed("sha256sums", "SHA256SUMS.txt", sha256sumsSeed());

console.log(`Seed corpus written to ${corpusRoot}`);
