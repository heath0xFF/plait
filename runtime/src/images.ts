// Thread-owned originals. Socket requests carry bounded chunks, never paths.
import { createHash } from 'node:crypto';
import type { DatabaseSync } from 'node:sqlite';
import { fail } from './protocol.ts';

export const IMAGE_BYTES = 5 * 1024 * 1024;
export const MESSAGE_IMAGE_BYTES = 5 * 1024 * 1024;
export const IMAGE_COUNT = 4;
export const IMAGE_CHUNK = 32768;
export const IMAGE_READ_CHUNK = 512 * 1024;
export const IMAGE_PIXELS = 16 * 1024 * 1024;
export type ImageRef = { id: string; media_type: 'image/png' | 'image/jpeg'; size: number; width: number; height: number };
export type ImageData = { ref: ImageRef; data: string };
const invalid = (message: string): never => fail('invalid_request', message);

/** Reads dimensions without decompressing untrusted pixels. Full decoding is the desktop's job. */
export function describe(bytes: Buffer): Omit<ImageRef, 'id'> {
  if (!bytes.length || bytes.length > IMAGE_BYTES) invalid('Each image must be at most 5 MiB.');
  let media_type: ImageRef['media_type'] = 'image/png'; let width = 0; let height = 0;
  if (bytes.length >= 33 && bytes.subarray(0, 8).equals(Buffer.from([137,80,78,71,13,10,26,10])) && bytes.toString('ascii', 12, 16) === 'IHDR' && bytes.readUInt32BE(8) === 13) {
    media_type = 'image/png'; width = bytes.readUInt32BE(16); height = bytes.readUInt32BE(20);
  } else if (bytes[0] === 255 && bytes[1] === 216) {
    media_type = 'image/jpeg'; let offset = 2;
    while (offset + 4 <= bytes.length) {
      if (bytes[offset++] !== 255) break;
      while (bytes[offset] === 255) offset++;
      const marker = bytes[offset++];
      if (marker === 217 || marker === 218 || offset + 2 > bytes.length) break;
      const length = bytes.readUInt16BE(offset);
      if (length < 2 || offset + length > bytes.length) break;
      if ([192,193,194,195,197,198,199,201,202,203,205,206,207].includes(marker) && length >= 8) {
        height = bytes.readUInt16BE(offset + 3); width = bytes.readUInt16BE(offset + 5); break;
      }
      offset += length;
    }
  } else invalid('Only PNG and JPEG images are supported.');
  if (!width || !height) invalid('Image dimensions could not be read.');
  if (width > 8192 || height > 8192 || width * height > IMAGE_PIXELS) invalid('Images must fit within 8192 pixels per side and 16,777,216 pixels total.');
  return { media_type, size: bytes.length, width, height };
}

export class Images {
  private db: DatabaseSync;
  constructor(db: DatabaseSync) {
    this.db = db;
    db.exec(`CREATE TABLE IF NOT EXISTS images(thread TEXT NOT NULL, id TEXT NOT NULL, total INTEGER NOT NULL, bytes BLOB NOT NULL,
      metadata TEXT, used INTEGER NOT NULL DEFAULT 0, created INTEGER NOT NULL, PRIMARY KEY(thread,id));
      CREATE TABLE IF NOT EXISTS message_images(thread TEXT NOT NULL, request TEXT NOT NULL, ids TEXT NOT NULL, PRIMARY KEY(thread,request));`);
    this.prune();
  }
  private prune() { this.db.prepare('DELETE FROM images WHERE used=0 AND created<?').run(Date.now() - 86400000); }
  put(thread: string, id: string, offset: number, total: number, data: string) {
    if (!/^[a-f0-9]{64}$/.test(id) || !Number.isSafeInteger(total) || total < 1 || total > IMAGE_BYTES || !Number.isSafeInteger(offset) || offset < 0) invalid('Invalid image upload or image exceeds 5 MiB.');
    if (typeof data !== 'string' || data.length > Math.ceil(IMAGE_CHUNK / 3) * 4 || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(data)) invalid('Invalid image chunk.');
    const chunk = Buffer.from(data, 'base64');
    if (!chunk.length || chunk.length > IMAGE_CHUNK || offset + chunk.length > total || chunk.toString('base64') !== data) invalid('Invalid image chunk size.');
    this.prune();
    const row = this.db.prepare('SELECT total,bytes,metadata FROM images WHERE thread=? AND id=?').get(thread, id);
    if (row && Number(row.total) !== total) invalid('Image size changed during upload.');
    const existing = row ? Buffer.from(row.bytes as Uint8Array) : Buffer.alloc(0);
    if (offset < existing.length) {
      if (!existing.subarray(offset, offset + chunk.length).equals(chunk)) invalid('Image chunk differs from the saved bytes.');
      return { offset: existing.length, image: row?.metadata ? JSON.parse(String(row.metadata)) : null };
    }
    if (offset !== existing.length) invalid('Image chunks must be uploaded in order.');
    if (!row) {
      const count = Number(this.db.prepare('SELECT COUNT(*) AS n FROM images WHERE used=0 AND thread=?').get(thread)!.n);
      const all = Number(this.db.prepare('SELECT COUNT(*) AS n FROM images WHERE used=0').get()!.n);
      if (count >= 16 || all >= 128) fail('busy', 'Too many unattached images. Remove unused images or try after a day.');
    }
    const bytes = Buffer.concat([existing, chunk]); let image: ImageRef | null = null;
    if (bytes.length === total) {
      if (createHash('sha256').update(bytes).digest('hex') !== id) invalid('Image bytes do not match their ID.');
      image = { id, ...describe(bytes) };
    }
    this.db.prepare(`INSERT INTO images(thread,id,total,bytes,metadata,created) VALUES(?,?,?,?,?,?)
      ON CONFLICT(thread,id) DO UPDATE SET bytes=excluded.bytes,metadata=excluded.metadata`).run(thread, id, total, bytes, image ? JSON.stringify(image) : null, Date.now());
    return { offset: bytes.length, image };
  }
  refs(thread: string, ids: string[]): ImageRef[] {
    if (!Array.isArray(ids) || ids.length > IMAGE_COUNT || new Set(ids).size !== ids.length) invalid('A message can have at most 4 different images.');
    const refs = ids.map(id => {
      const row = this.db.prepare('SELECT metadata FROM images WHERE thread=? AND id=?').get(thread, id);
      if (!row?.metadata) fail('not_found', 'An image has not finished uploading to this thread.');
      return JSON.parse(String(row.metadata)) as ImageRef;
    });
    if (refs.reduce((sum, ref) => sum + ref.size, 0) > MESSAGE_IMAGE_BYTES) invalid('Images in a message must total at most 5 MiB.');
    return refs;
  }
  attach(thread: string, request: string, ids: string[]) {
    if (!ids.length) return;
    this.refs(thread, ids);
    this.db.prepare('INSERT INTO message_images VALUES(?,?,?)').run(thread, request, JSON.stringify(ids));
    for (const id of ids) this.db.prepare('UPDATE images SET used=1 WHERE thread=? AND id=?').run(thread, id);
  }
  /** Cancelled queued inputs never entered the conversation. Keep a day for Edit/retry recovery. */
  detach(thread: string, request: string) {
    const row = this.db.prepare('DELETE FROM message_images WHERE thread=? AND request=? RETURNING ids').get(thread, request);
    if (!row) return;
    for (const id of JSON.parse(String(row.ids)) as string[]) {
      this.db.prepare(`UPDATE images SET used=0,created=? WHERE thread=? AND id=? AND NOT EXISTS
        (SELECT 1 FROM message_images, json_each(message_images.ids) WHERE thread=? AND json_each.value=?)`)
        .run(Date.now(), thread, id, thread, id);
    }
  }
  forRequest(thread: string, request: string): ImageRef[] {
    const row = this.db.prepare('SELECT ids FROM message_images WHERE thread=? AND request=?').get(thread, request);
    return row ? this.refs(thread, JSON.parse(String(row.ids))) : [];
  }
  data(thread: string, refs: ImageRef[]): ImageData[] {
    return refs.map(ref => ({ ref, data: Buffer.from(this.db.prepare('SELECT bytes FROM images WHERE thread=? AND id=? AND metadata IS NOT NULL').get(thread, ref.id)!.bytes as Uint8Array).toString('base64') }));
  }
  read(thread: string, id: string, offset: number) {
    const image = this.refs(thread, [id])[0];
    if (!Number.isSafeInteger(offset) || offset < 0 || offset >= image.size) invalid('Invalid image offset.');
    const row = this.db.prepare('SELECT substr(bytes,?,?) AS chunk FROM images WHERE thread=? AND id=?').get(offset + 1, IMAGE_READ_CHUNK, thread, id)!;
    return { image, offset, data: Buffer.from(row.chunk as Uint8Array).toString('base64') };
  }
}
