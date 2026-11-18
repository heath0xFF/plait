// Run via scripts/bench-images.sh: real local host, offline store, production Rust fetch/decode.
import assert from 'node:assert/strict';
import { crc32, deflateSync } from 'node:zlib';
import { createHash } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import { resolve, join } from 'node:path';
import { IMAGE_BYTES, IMAGE_CHUNK, describe } from '../src/images.ts';
process.env.PLAIT_TEST_RUNTIME=resolve('test/support/image-bench-runtime.ts');
const {setup,request,exec}=await import('./support/host.ts');
function chunk(type:string,data:Buffer) {
  const tag=Buffer.from(type); const length=Buffer.alloc(4); length.writeUInt32BE(data.length);
  const crc=Buffer.alloc(4); crc.writeUInt32BE(crc32(Buffer.concat([tag,data])));
  return Buffer.concat([length,tag,data,crc]);
}
function png(size:number) {
  const width=3840,height=2160;
  const header=Buffer.alloc(13); header.writeUInt32BE(width); header.writeUInt32BE(height,4); header[8]=8; header[9]=2;
  const pixels=Buffer.alloc((width*3+1)*height);
  for(let y=0;y<height;y++) for(let x=0;x<width;x++) {
    const offset=y*(width*3+1)+1+x*3;
    pixels[offset]=x%256; pixels[offset+1]=y%256; pixels[offset+2]=(x+y)%256;
  }
  const start=Buffer.concat([Buffer.from([137,80,78,71,13,10,26,10]),chunk('IHDR',header),chunk('IDAT',deflateSync(pixels))]);
  const end=chunk('IEND',Buffer.alloc(0));
  const padding=Buffer.alloc(size-start.length-end.length-12,120); padding.write('padding\0');
  const bytes=Buffer.concat([start,chunk('tEXt',padding),end]); assert.equal(bytes.length,size); return bytes;
}
for(const [size,mode] of [[2*1024*1024,'old-chunks'],[IMAGE_BYTES,'old-chunks'],[IMAGE_BYTES,'new-chunks']] as const) {
  const cleanup:(()=>Promise<void>)[]=[];
  try {
    const h=await setup({after:fn=>cleanup.push(fn)},mode,false,false); const thread=await h.create();
    const bytes=png(size); const id=createHash('sha256').update(bytes).digest('hex');
    for(let offset=0;offset<size;offset+=IMAGE_CHUNK) {
      const reply=await request(h.store,{op:'image_put',protocol_version:6,thread_id:thread.id,image_id:id,total:size,offset,data:bytes.subarray(offset,offset+IMAGE_CHUNK).toString('base64')});
      assert.equal(reply.ok,true,JSON.stringify(reply));
    }
    const result=await exec('cargo',['test','--locked','--release','-p','plait','ui::images::thumbnail_transfer_bench','--','--exact','--ignored','--nocapture'],{cwd:resolve('..'),env:{...process.env,PLAIT_HOST_STORE:h.store,PLAIT_IMAGE_BENCH:JSON.stringify({thread:thread.id,image:{id,...describe(bytes)}})},maxBuffer:1024*1024});
    const counts=(await readFile(join(h.store,'image-requests.jsonl'),'utf8')).trim().split('\n').map(Number);
    const expected=Math.ceil(size/(mode==='old-chunks'?32768:512*1024));
    assert.equal(counts.length,12); assert.ok(counts.every(count=>count===expected));
    console.log(`${size} bytes; ${mode}; ${expected} requests/thumbnail; ${result.stdout.split('\n').find(line=>line.startsWith('THUMBNAIL_BENCH'))}`);
  } finally { for(const fn of cleanup.reverse()) await fn(); }
}
