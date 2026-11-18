import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createHash } from 'node:crypto';
import { readFile, mkdir } from 'node:fs/promises';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, IMAGE_BYTES, IMAGE_CHUNK, IMAGE_READ_CHUNK, MESSAGE_IMAGE_BYTES } from '../src/images.ts';
import { Store } from '../src/store.ts';
import { validate, view } from '../src/protocol.ts';
import type { Snapshot } from '../src/protocol.ts';
import { codex, claude, models } from './support/record-runtime.ts';
import { setup as socketHost, request, until } from './support/host.ts';
import { setup } from './support/safety.ts';
import { createModels } from '@earendil-works/pi-ai/models';
import { fauxProvider } from '@earendil-works/pi-ai/providers/faux';
import { MAX_LINE_BYTES } from '../src/codex/server.ts';
import { Runtime } from '../src/runtime.ts';

const png = readFileSync(new URL('./support/images/0.png', import.meta.url));
const id = (bytes: Buffer) => createHash('sha256').update(bytes).digest('hex');
async function upload(call: (input: any) => Promise<any>, thread: string, bytes = png) {
  let image;
  for (let offset=0; offset<bytes.length; offset+=IMAGE_CHUNK) {
    const reply=await call({ op:'image_put',protocol_version:6,thread_id:thread,image_id:id(bytes),offset,total:bytes.length,data:bytes.subarray(offset,offset+IMAGE_CHUNK).toString('base64') });
    assert.equal(reply.ok,true,JSON.stringify(reply)); image=reply.result.image;
  }
  return image;
}
test('version 6 images stay out of version 5 messages and validation',()=>{
  const image={id:id(png),...describe(png)};
  const snapshot: Snapshot={id:'t',checkout:'/repo',model:{provider:'p',model_id:'m'},status:'idle',messages:[{id:'r',role:'user',text:'look',images:[image]}],tools:[],live_text:'',pending_approval:null,error:null,truncated:false,image_input:'yes'};
  assert.equal('images' in view(snapshot,5).messages[0],false);
  assert.equal('image_input' in view(snapshot,5),false);
  assert.deepEqual((view(snapshot,6).messages[0] as any).images,[image]);
  const message={op:'submit',thread_id:'t',request_id:'r',text:'',images:[image.id]};
  assert.doesNotThrow(()=>validate({...message,protocol_version:6}));
  assert.throws(()=>validate({...message,protocol_version:5}));
  assert.throws(()=>validate({...message,protocol_version:6,images:Array(5).fill(image.id)}));
});
test('bounded image storage validates bytes, dimensions, IDs, chunks, count and message totals',async t=>{
  const {base}=await setup(t); const store=new Store(base); t.after(()=>store.close()); const images=store.images;
  assert.equal(describe(png).media_type,'image/png');
  assert.equal(describe(Buffer.from([255,216,255,192,0,11,8,0,16,0,32,1,1,17,0,255,217])).media_type,'image/jpeg');
  assert.throws(()=>describe(Buffer.from('not a PNG.jpg')),/PNG and JPEG/);
  assert.throws(()=>describe(Buffer.alloc(IMAGE_BYTES+1)),/5 MiB/);
  const enormous=Buffer.from(png); enormous.writeUInt32BE(100000,16); assert.throws(()=>describe(enormous),/8192/);
  assert.throws(()=>images.put('t',id(png),0,IMAGE_BYTES+1,png.toString('base64')),/5 MiB/);
  assert.throws(()=>images.put('t',id(png),1,png.length,png.toString('base64')),/chunk/);
  assert.throws(()=>images.put('t','a'.repeat(64),0,png.length,png.toString('base64')),/ID/);
  const ids: string[]=[];
  for(let n=0;n<2;n++) {
    const bytes=Buffer.concat([png,Buffer.alloc(IMAGE_BYTES-png.length,n)]); ids.push(id(bytes));
    for(let offset=0;offset<bytes.length;offset+=IMAGE_CHUNK) images.put('t',ids[n],offset,bytes.length,bytes.subarray(offset,offset+IMAGE_CHUNK).toString('base64'));
  }
  assert.throws(()=>images.refs('t',ids),/5 MiB/);
  assert.equal(images.refs('t',[ids[0]])[0].size, IMAGE_BYTES);
  let requests = 0; let total = 0;
  while (total < IMAGE_BYTES) {
    const reply = {ok:true,result:images.read('t',ids[0],total)};
    const part = Buffer.from(reply.result.data,'base64');
    assert.equal(part.length, Math.min(IMAGE_READ_CHUNK,IMAGE_BYTES-total));
    assert.ok(Buffer.byteLength(JSON.stringify(reply))+1 < 1048576);
    total += part.length; requests++;
  }
  assert.equal(requests,10);
  assert.throws(()=>images.put('too-large-chunk',id(png),0,IMAGE_BYTES,Buffer.alloc(IMAGE_CHUNK+1).toString('base64')),/chunk/);

  assert.throws(()=>images.refs('t',Array(5).fill(ids[0])),/4 different/);
  assert.throws(()=>images.refs('other',[ids[0]]),/uploading/);
  assert.throws(()=>images.read('t',ids[0],IMAGE_BYTES),/offset/);
});
for (const [name,make] of [['codex',codex],['claude',claude]] as const) test(`${name} receives original images, preserves references on reopen, and keeps version 5 history unchanged`,async t=>{
  const h=await make(t); const thread=await h.create();
  const call=(input:any)=>h.call(input,6); const image=await upload(call,thread.id);
  assert.equal((await call({op:'submit',thread_id:thread.id,request_id:'image-request',text:'look at this',images:[image.id]})).ok,true);
  await h.finish(thread.id);
  const snapshot=h.runtime.snapshot(thread.id); assert.deepEqual(snapshot.messages.find(m=>m.id==='request-image-request')?.images,[image]);
  const history=await call({op:'history',thread_id:thread.id}); assert.deepEqual(history.result.items.find((m:any)=>m.id==='request-image-request')?.images,[image]);
  const old=await h.call({op:'history',thread_id:thread.id},5); assert.equal(old.result.items.some((m:any)=>'images' in m),false);
  const log=name==='codex'?join(h.store,'frames.jsonl'):join(h.store,'claude-images.jsonl');
  if(name==='codex') {
    const frames=(await readFile(log,'utf8')).trim().split('\n').map(line=>JSON.parse(line));
    const content=frames.find(frame=>frame.method==='turn/start').params.input;
    assert.deepEqual(content[1],{type:'image',url:`data:image/png;base64,${png.toString('base64')}`});
  } else {
    const frames=(await readFile(log,'utf8')).trim().split('\n').map(line=>JSON.parse(line));
    const content=frames.filter(frame=>frame.stdin).map(frame=>JSON.parse(frame.stdin)).find(frame=>frame.type==='user').message.content;
    assert.deepEqual(content[1],{type:'image',source:{type:'base64',media_type:'image/png',data:png.toString('base64')}});
  }
  await h.close();
  const reopened=new Store(h.store); assert.deepEqual(reopened.images.forRequest(thread.id,'image-request'),[image]);
  assert.equal(reopened.images.read(thread.id,image.id,0).data,png.toString('base64')); reopened.close();
  const resumed = h.reopen();
  assert.deepEqual(resumed.runtime.snapshot(thread.id).messages.find(m=>m.id==='request-image-request')?.images,[image]);
  assert.equal((await resumed.call({op:'submit',thread_id:thread.id,request_id:'image-after-restart',text:'again',images:[image.id]},6)).ok,true);
  await resumed.finish(thread.id);
  const replayed=(await readFile(log,'utf8')).trim().split('\n').map(line=>JSON.parse(line));
  const inputs=name==='codex'?replayed.filter(f=>f.method==='turn/start').map(f=>f.params.input):replayed.filter(f=>f.stdin).map(f=>JSON.parse(f.stdin)).filter(f=>f.type==='user').map(f=>f.message.content);
  assert.equal(inputs.length,2);
  assert.deepEqual(inputs[0][1],inputs[1][1]);

});
test('Pi images survive a real host restart and replay through the scripted provider',async t=>{
  const h=await socketHost(t,'images'); const thread=await h.create(); const call=(input:any)=>request(h.store,{protocol_version:6,...input}) as Promise<any>;
  const image=await upload(call,thread.id);
  const submission={op:'submit',thread_id:thread.id,request_id:'image-request',text:'look',images:[image.id]};
  assert.equal((await call(submission)).ok,true);
  await until(()=>call({op:'watch',thread_id:thread.id}),reply=>reply.result.thread.status==='idle');
  const contexts=(await readFile(join(h.store,'model-contexts.jsonl'),'utf8')).trim().split('\n').map(line=>JSON.parse(line));
  assert.deepEqual(contexts[0].find((m:any)=>m.role==='user').content[1],{type:'image',mimeType:'image/png',data:png.toString('base64')});
  h.host.kill('SIGTERM'); await until(async()=>h.host.exitCode!==null||h.host.signalCode!==null,done=>done); await h.start();
  assert.equal((await call({op:'image_read',thread_id:thread.id,image_id:image.id,offset:0})).result.data,png.toString('base64'));
  assert.deepEqual((await call({op:'watch',thread_id:thread.id})).result.thread.messages.find((m:any)=>m.id==='request-image-request').images,[image]);
  assert.equal((await call(submission)).ok,true);
  assert.equal((await call({...submission,request_id:'after-restart',text:'again'})).ok,true);
  await until(()=>call({op:'watch',thread_id:thread.id}),reply=>reply.result.thread.status==='idle');
  const replayed=(await readFile(join(h.store,'model-contexts.jsonl'),'utf8')).trim().split('\n').map(line=>JSON.parse(line));
  assert.equal(replayed.length,2);
  assert.deepEqual(replayed[1].filter((m:any)=>m.role==='user').map((m:any)=>m.content[1]),Array(2).fill(contexts[0].find((m:any)=>m.role==='user').content[1]));
});
test('a text-only Pi model refuses images without accepting the message',async t=>{
  const {root,base}=await setup(t); const directory=join(base,'store'); await mkdir(directory); const catalog=createModels(); catalog.setProvider(fauxProvider({models:[{id:'faux-1',name:'Text',input:['text']}]}).provider); const runtime=new Runtime(directory,catalog,new Set(['faux'])); t.after(()=>runtime.close());
  const call=(input:any)=>runtime.reply({protocol_version:6,...input}) as Promise<any>;
  const thread=(await call({op:'create_thread',checkout:root,provider:'faux',model_id:'faux-1',request_id:'create'})).result.thread;
  const image=await upload(call,thread.id);
  assert.equal((await call({op:'submit',thread_id:thread.id,request_id:'no-images',text:'look',images:[image.id]})).error?.code,'model_unavailable');
  assert.equal(runtime.store.inputs(thread.id).length,0);
});

// Use the exact 65,000-byte runtime request limit, including image IDs and escaping.
function largestSubmit(thread: string, image: string) {
  const request = { op:'submit',protocol_version:6,thread_id:thread,request_id:'largest-image',text:'',images:[image] };
  request.text = 'x'.repeat(65000 - Buffer.byteLength(JSON.stringify(request)));
  assert.equal(Buffer.byteLength(JSON.stringify(request)),65000);
  assert.doesNotThrow(()=>validate(request));
  assert.throws(()=>validate({...request,text:request.text+'x'}),/too large/);
  return request;
}
for (const [name, make] of [['codex',codex],['claude',claude]] as const) test(`${name} carries 5 MiB and maximum message text with frame headroom`,async t=>{
  const h=await make(t); const thread=await h.create();
  const bytes=Buffer.concat([png,Buffer.alloc(IMAGE_BYTES-png.length)]);
  const image=await upload(input=>h.call(input,6),thread.id,bytes);
  const submission=largestSubmit(thread.id,image.id);
  assert.equal((await h.call(submission,6)).ok,true);
  await h.finish(thread.id);
  const log=name==='codex'?'frames.jsonl':'claude-images.jsonl';
  const frames=(await readFile(join(h.store,log),'utf8')).trim().split('\n').map(line=>JSON.parse(line));
  const frame=name==='codex'?frames.find(f=>f.method==='turn/start'):frames.filter(f=>f.stdin).map(f=>JSON.parse(f.stdin)).find(f=>f.type==='user');
  const content=name==='codex'?frame.params.input:frame.message.content;
  assert.equal(content[0].text,submission.text);
  assert.equal(name==='codex'?content[1].url:`data:image/png;base64,${content[1].source.data}`,`data:image/png;base64,${bytes.toString('base64')}`);
  assert.ok(Buffer.byteLength(JSON.stringify(frame))+1 < MAX_LINE_BYTES - 1024*1024);
});
test('Pi carries 5 MiB plus maximum message text through the real host',async t=>{
  const h=await socketHost(t,'images'); const thread=await h.create();
  const bytes=Buffer.concat([png,Buffer.alloc(IMAGE_BYTES-png.length)]);
  const image=await upload(input=>request(h.store,input),thread.id,bytes);
  const submission=largestSubmit(thread.id,image.id);
  assert.equal((await request(h.store,submission)).ok,true);
  await until(()=>request(h.store,{op:'watch',protocol_version:6,thread_id:thread.id}),reply=>reply.result.thread.status==='idle');
  const contexts=(await readFile(join(h.store,'model-contexts.jsonl'),'utf8')).trim().split('\n').map(line=>JSON.parse(line));
  const message=contexts[0].find((m:any)=>m.role==='user');
  assert.equal(message.content[0].text,submission.text);
  assert.equal(message.content[1].data,bytes.toString('base64'));
  assert.ok(Buffer.byteLength(JSON.stringify(message))+1 < MAX_LINE_BYTES - 1024*1024);
});
test('four base64 blocks plus worst allowed text fit the harness line bound',()=>{
  assert.equal(IMAGE_BYTES,5*1024*1024); assert.equal(MESSAGE_IMAGE_BYTES,IMAGE_BYTES);
  const sizes=[1,1,1,MESSAGE_IMAGE_BYTES-3];
  const encoded=sizes.reduce((sum,size)=>sum+4*Math.ceil(size/3),0);
  assert.equal(encoded,6990516);
  // More than a MiB remains even reserving 64 KiB for serialized text and 4 KiB for framing.
  assert.ok(encoded+65536+4096+1 < MAX_LINE_BYTES-1024*1024);
});
