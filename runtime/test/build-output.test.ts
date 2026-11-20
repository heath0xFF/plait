import assert from 'node:assert/strict';
import { chmod, mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { randomUUID } from 'node:crypto';
import { test } from 'node:test';
import { buildHelper, type BuildInfo, type Outcome } from '../src/build-output.ts';
import { acquireCheckout } from '../src/safety.ts';
import type { Operation } from '../src/worktrees.ts';
import { codex } from './support/record-runtime.ts';
import { exec } from './support/safety.ts';

type Harness = Awaited<ReturnType<typeof codex>>;
async function setup(h: Harness) {
  await chmod(h.store, 0o700);
  await writeFile(join(h.root,'.gitignore'),'target/\ncustom/\nother/\n.env\n');
  await writeFile(join(h.root,'Cargo.toml'),'[package]\nname="fixture"\nversion="0.1.0"\n');
  await exec('/usr/bin/git',['-C',h.root,'add','.']);
  await exec('/usr/bin/git',['-C',h.root,'-c','user.name=Test','-c','user.email=test@example.invalid','commit','-qm','initial']);
  for (const path of ['target','custom','other']) { await mkdir(join(h.root,path)); await writeFile(join(h.root,path,'artifact'),path); }
  await writeFile(join(h.root,'.env'),'secret');
}
async function info(h: Harness): Promise<BuildInfo> { const r:any=await h.runtime.reply({op:'build_output_info',checkout:h.root}); assert.equal(r.ok,true,JSON.stringify(r)); return r.result; }
async function approve(h: Harness, paths: string[]) {
  const before=await info(h);
  const r:any=await h.runtime.reply({op:'set_build_output',checkout:h.root,identity:before.policy.identity,revision:before.policy.revision,paths});
  assert.equal(r.ok,true,JSON.stringify(r)); return r.result as BuildInfo;
}
async function create(h: Harness, id: string) {
  const r:any=await h.runtime.reply({op:'worktree_info',checkout:h.root}); assert.equal(r.ok,true,JSON.stringify(r));
  const request={op:'create_worktree_thread',request_id:id,parent:r.result.parent,harness:'codex',provider:'codex',model_id:'gpt-fake',mode:'ask',approval:'ask'};
  const start=await h.runtime.reply(request); assert.equal(start.ok,true,JSON.stringify(start)); return request;
}
async function ready(h: Harness,id:string):Promise<Operation> {
  const end=Date.now()+20000;
  for (;;) { const r:any=await h.runtime.reply({op:'worktree_status',request_id:id}); const op=r.result?.operation; if(op?.state!=='preparing') { assert.equal(op?.state,'ready',JSON.stringify(r)); return op; } assert.ok(Date.now()<end,'creation timed out'); await new Promise(r=>setTimeout(r,10)); }
}
async function warmState(h:Harness,id:string) { const r:any=await h.runtime.reply({op:'watch_build_output',request_id:id,kind:'worktree'}); assert.equal(r.ok,true,JSON.stringify(r)); return r.result; }

test('host approvals survive tracked edits and old inventory replies stay compatible',async t=>{
  const h=await codex(t); await setup(h); const initial=await info(h);
  assert.deepEqual(initial.policy.paths,[]); assert.deepEqual(initial.suggestions,['target']);
  await approve(h,['custom']); await writeFile(join(h.root,'Cargo.toml'),'[build_output]\napproved=["other"]\n');
  assert.deepEqual((await info(h)).policy.paths,['custom']);
  const old:any=await h.runtime.reply({op:'project_worktrees',checkout:h.root}); assert.equal(old.ok,true,JSON.stringify(old));
  assert.equal('worktree_build_output_v1' in old.result,false);
  assert.equal('build_directories' in old.result.checkouts[0],false);
  const stale:any=await h.runtime.reply({op:'set_build_output',checkout:h.root,identity:initial.policy.identity,revision:0,paths:['other']}); assert.equal(stale.ok,false);
  for (const paths of [['.env'],['../store'],['.git'],['custom','custom/nested']]) {
    const current=await info(h); const bad:any=await h.runtime.reply({op:'set_build_output',checkout:h.root,identity:current.policy.identity,revision:current.policy.revision,paths}); assert.equal(bad.ok,false,JSON.stringify(bad));
  }
});
test('unapproved projects start cold and never copy adjacent ignored data',async t=>{
  const h=await codex(t); await setup(h); await create(h,'cold'); const op=await ready(h,'cold');
  const state=await warmState(h,'cold'); assert.equal(state.phase,'cold'); assert.match(state.outcome.reason,/No build directories are approved/);
  for (const path of ['target','custom','other','.env']) await assert.rejects(readFile(join(op.worktree!.path,path,'artifact')));
});
test('approved output warms the checkout before readiness and repeats idempotently',{skip:process.platform!=='darwin'},async t=>{
  const h=await codex(t); await setup(h); await approve(h,['target','custom']);
  const request=await create(h,'warm'); const op=await ready(h,'warm'); const state=await warmState(h,'warm');
  assert.equal(state.phase,'warm',JSON.stringify(state)); assert.equal(state.outcome.counts.files,2);
  assert.equal(await readFile(join(op.worktree!.path,'target/artifact'),'utf8'),'target');
  assert.equal(await readFile(join(op.worktree!.path,'custom/artifact'),'utf8'),'custom');
  await assert.rejects(readFile(join(op.worktree!.path,'.env'))); await assert.rejects(readFile(join(op.worktree!.path,'other/artifact')));
  const repeat:any=await h.runtime.reply(request); assert.equal(repeat.result.operation.thread_id,op.thread_id);
  const rows:any=await h.runtime.reply({op:'project_worktrees_build',checkout:h.root}); assert.equal(rows.result.worktree_build_output_v1,true);
  const row=rows.result.checkouts.find((row:any)=>row.thread_id===op.thread_id); assert.equal(row.build_output.warm,true); assert.equal(row.preservation.changed,0); assert.equal(row.preservation.build_output,2);
  await writeFile(join(op.worktree!.path,'target/artifact'),'clone'); assert.equal(await readFile(join(h.root,'target/artifact'),'utf8'),'target');
});
test('busy checkout uses an immutable seed and later writes do not disturb it',{skip:process.platform!=='darwin'},async t=>{
  const h=await codex(t); await setup(h); await approve(h,['custom']); await new Promise(r=>setImmediate(r)); await h.runtime.worktrees.builds.yieldToRun(h.root);
  const policy=(await info(h)).policy;
  await buildHelper<Outcome>({action:'seed',store:h.store,root:h.root,identity:policy.identity,generation:randomUUID(),source_cache:null});
  const lease=await acquireCheckout(h.root);
  try { await writeFile(join(h.root,'custom/artifact'),'moving source'); await create(h,'seeded'); const op=await ready(h,'seeded'); assert.equal((await warmState(h,'seeded')).phase,'warm'); assert.equal(await readFile(join(op.worktree!.path,'custom/artifact'),'utf8'),'custom'); }
  finally { await lease.close(); }
});
test('Skip before cloning admits a cold worktree with no partial directory',async t=>{
  const h=await codex(t); await setup(h); await approve(h,['target']); await create(h,'skip');
  const skipped:any=await h.runtime.reply({op:'skip_build_output',request_id:'skip',kind:'worktree'}); assert.equal(skipped.ok,true,JSON.stringify(skipped));
  const op=await ready(h,'skip'); const state=await warmState(h,'skip'); assert.equal(state.phase,'cold',JSON.stringify(state)); assert.match(state.outcome.reason,/skipped/);
  await assert.rejects(readFile(join(op.worktree!.path,'target/artifact')));
});
test('removal consults current host policy and still preserves an env file',{skip:process.platform!=='darwin'},async t=>{
  const h=await codex(t); await setup(h); await approve(h,['custom']); await create(h,'removal'); const op=await ready(h,'removal');
  const remove=async(id:string)=>{
    const result=await h.runtime.reply({op:'remove_worktree',request_id:id,creation_id:'removal',identity:op.worktree!.identity,allow_unpushed:true}); assert.equal(result.ok,true,JSON.stringify(result));
    for (let n=0;n<2000;n++) { const state:any=await h.runtime.reply({op:'worktree_status',request_id:id}); if(state.result.operation.state!=='removing') return state.result.operation; await new Promise(r=>setTimeout(r,10)); }
    assert.fail('removal timed out');
  };
  await approve(h,[]);
  const revoked:any=await remove('revoked'); assert.equal(revoked.state,'ready'); assert.match(revoked.error.message,/ignored files/);
  await approve(h,['custom']); await writeFile(join(op.worktree!.path,'.env'),'keep');
  const env:any=await remove('env'); assert.equal(env.state,'ready'); assert.ok(env.error); assert.equal(await readFile(join(op.worktree!.path,'.env'),'utf8'),'keep');
});

test('real host restart, Skip and lease preemption interrupt cloned output safely',{skip:process.platform!=='darwin',timeout:60000},async t=>{
  const { setup: hostSetup, request, childPids, until }=await import('./support/host.ts');
  const { connect }=await import('node:net');
  const f=await hostSetup(t);
  await writeFile(join(f.repo,'.gitignore'),'custom/\n.env\n');
  await exec('/usr/bin/git',['-C',f.repo,'add','.gitignore']);
  await exec('/usr/bin/git',['-C',f.repo,'-c','user.name=Test','-c','user.email=test@example.invalid','commit','-qm','ignore output']);
  await mkdir(join(f.repo,'custom'));
  for(let bucket=0;bucket<100;bucket++) {
    const dir=join(f.repo,'custom',String(bucket)); await mkdir(dir);
    await Promise.all(Array.from({length:200},(_,i)=>writeFile(join(dir,`artifact-${i}`),'output')));
  }
  const info:any=await request(f.store,{op:'build_output_info',checkout:f.repo}); assert.equal(info.ok,true,JSON.stringify(info));
  const approved=await request(f.store,{op:'set_build_output',checkout:f.repo,identity:info.result.policy.identity,revision:0,paths:['custom']}); assert.equal(approved.ok,true,JSON.stringify(approved));
  const parent:any=await request(f.store,{op:'worktree_info',checkout:f.repo});
  const [node]=await childPids(f.host.pid!); assert.ok(node);
  const socket=connect(join(f.store,'host.sock')); t.after(async()=>{socket.destroy();});
  let buffer=''; let killed=false; let count=0;
  socket.setEncoding('utf8'); socket.on('error',()=>{});
  socket.on('connect',()=>socket.write(JSON.stringify({op:'watch_build_output',request_id:'crash-clone',kind:'worktree'})+'\n'));
  socket.on('data',data=>{
    buffer+=data;
    for(;;) { const end=buffer.indexOf('\n'); if(end<0)break; const message=JSON.parse(buffer.slice(0,end));buffer=buffer.slice(end+1); const state=message.build_output??message.result;
      if(!killed&&state?.phase==='cloning'&&state.files>0) { count=state.files; killed=true; process.kill(node,'SIGKILL'); }
    }
  });
  const accepted:any=await request(f.store,{op:'create_worktree_thread',request_id:'crash-clone',parent:parent.result.parent,harness:'pi',provider:'faux',model_id:'faux-1',approval:'ask'});
  assert.equal(accepted.ok,true,JSON.stringify(accepted));
  await until(async()=>killed,Boolean,20000); assert.ok(count>0&&count<20000,`interrupted at ${count}`);
  await until(async()=>f.host.exitCode!==null||f.host.signalCode!==null,Boolean);
  await f.start();
  const recovered:any=await until(async()=>(await request(f.store,{op:'worktree_status',request_id:'crash-clone'}) as any).result.operation,(op:any)=>op.state!=='preparing');
  assert.equal(recovered.state,'ready',JSON.stringify(recovered)); assert.equal(recovered.thread_id,accepted.result.operation.thread_id);
  const state:any=await request(f.store,{op:'watch_build_output',request_id:'crash-clone',kind:'worktree'}); assert.equal(state.result.phase,'cold'); assert.match(state.result.outcome.reason,/interrupted/);
  await assert.rejects(readFile(join(recovered.worktree.path,'custom/0/artifact-0')));
  const { readdir }=await import('node:fs/promises'); assert.equal((await readdir(recovered.worktree.path)).some(p=>p.startsWith('.plait-build-')),false);
  socket.destroy();
  const cancelSocket=connect(join(f.store,'host.sock')); t.after(async()=>{cancelSocket.destroy();});
  let cancelBuffer=''; let cancelling:Promise<unknown>|undefined;
  cancelSocket.setEncoding('utf8'); cancelSocket.on('error',()=>{});
  cancelSocket.on('connect',()=>cancelSocket.write(JSON.stringify({op:'watch_build_output',request_id:'cancel-clone',kind:'worktree'})+'\n'));
  cancelSocket.on('data',data=>{
    cancelBuffer+=data;
    for(;;) { const end=cancelBuffer.indexOf('\n'); if(end<0)break; const message=JSON.parse(cancelBuffer.slice(0,end));cancelBuffer=cancelBuffer.slice(end+1); const state=message.build_output??message.result;
      if(!cancelling&&state?.phase==='cloning'&&state.files>0) cancelling=request(f.store,{op:'skip_build_output',request_id:'cancel-clone',kind:'worktree'});
    }
  });
  const second:any=await request(f.store,{op:'create_worktree_thread',request_id:'cancel-clone',parent:parent.result.parent,harness:'pi',provider:'faux',model_id:'faux-1',approval:'ask'}); assert.equal(second.ok,true,JSON.stringify(second));
  await until(async()=>!!cancelling,Boolean,20000); await cancelling;
  const cancelled:any=await until(async()=>(await request(f.store,{op:'worktree_status',request_id:'cancel-clone'}) as any).result.operation,(op:any)=>op.state!=='preparing');
  assert.equal(cancelled.state,'ready',JSON.stringify(cancelled));
  const cold:any=await request(f.store,{op:'watch_build_output',request_id:'cancel-clone',kind:'worktree'}); assert.equal(cold.result.phase,'cold'); assert.match(cold.result.outcome.reason,/skipped/);
  assert.equal((await readdir(cancelled.worktree.path)).some(p=>p.startsWith('.plait-build-')||p==='custom'),false);
  // This caller has no BuildOutputs job map. The shared lease registry must
  // wake the other helper, just as it does for a run in another host store.
  let writer:Promise<{elapsed:number;error:unknown}>|undefined;
  const capture=buildHelper<Outcome>({action:'seed',store:f.store,root:f.repo,identity:info.result.policy.identity,generation:randomUUID(),source_cache:null},undefined,counts=>{
    if(!writer&&counts.files>0) { const start=Date.now(); writer=acquireCheckout(f.repo).then(async lease=>{await lease.close();return {elapsed:Date.now()-start,error:null};},error=>({elapsed:Date.now()-start,error})); }
  }).then(value=>({value,error:null}),error=>({value:null,error}));
  const interrupted=await capture; assert.ok(interrupted.error,'seed should yield instead of publishing'); assert.ok(writer);
  const {elapsed,error}=await writer; assert.equal(error,null); assert.ok(elapsed<1000,`seed yield took ${elapsed} ms`); console.log(`shared seed preemption: ${elapsed} ms`);
});
