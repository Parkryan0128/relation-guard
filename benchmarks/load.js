import http from 'k6/http';
import exec from 'k6/execution';
import { Counter, Trend } from 'k6/metrics';
const correct = new Counter('correct_responses');
const unexpected = new Counter('unexpected_responses');
const conflicts = new Counter('revision_conflicts');
const mismatches = new Counter('correctness_mismatches');
const successes = new Trend('successful_latency',true);
const cfg=JSON.parse(open(__ENV.CONFIG));
const statuses={};for(const status of [0,200,400,401,403,409,422,500,503,504])statuses[status]=new Counter('responses_'+status);
const profile=__ENV.PROFILE || 'single';
const rate=Number(__ENV.RATE || (profile==='batch'?10:100));
const duration=__ENV.DURATION || '300s';
export const options={scenarios:{load:{executor:'constant-arrival-rate',rate,timeUnit:'1s',duration,preAllocatedVUs:32,maxVUs:128}},summaryTrendStats:['avg','min','med','max','p(50)','p(95)','p(99)'],thresholds:{correctness_mismatches:['count==0']}};
function call(path,body,tenant){return http.post(`${cfg.url}/v1/tenants/${tenant.id}/${path}`,JSON.stringify(body),{headers:{'X-Actor-Id':tenant.actor || 'test-admin',Authorization:`Bearer ${tenant.token}`,'Content-Type':'application/json','Idempotency-Key':`load-${__VU}-${__ITER}-${Date.now()}`},timeout:'3s'});}
export default function(){
 const i=exec.scenario.iterationInTest;
 const tenant=cfg.tenants[profile==='mixed-distributed'?Math.floor(i/20)%cfg.tenants.length:0];
 if(profile.startsWith('mixed') && i%20===0){
  const r=http.get(`${cfg.url}/v1/tenants/${tenant.id}/revision`,{headers:{'X-Actor-Id':tenant.actor || 'test-admin',Authorization:`Bearer ${tenant.token}`},timeout:'3s'});
  if(r.status!==200){unexpected.add(1);return;}
  const resource={kind:'document',id:`d${i%cfg.documents}`};
  const response=call('write',{expected_revision:r.json().revision,operations:[{op:'put_relation',relationship:{resource,relation:'viewer',subject:{kind:'user',id:'u998'},subject_relation:''}}]},tenant);
  if(response.status===409){conflicts.add(1);return;}
  if(response.status!==200){unexpected.add(1);return;}correct.add(1);successes.add(response.timings.duration);return;
 }
 const allow=i%2===0;const d=i%cfg.documents;
 // All generated resources exist. Users 500..999 cannot match benchmark grants.
 const subject={kind:'user',id:allow?`u${d%(profile==='batch'?50:500)}`:'u999'};
 const checks=profile==='batch'?Array.from({length:100},(_,j)=>({resource:{kind:'document',id:`d${(d+j*50)%cfg.documents}`},action:'view'})):null;
 const response=call(checks?'batch-check':'check',checks?{subject,checks}:{subject,resource:{kind:'document',id:`d${d}`},action:'view'},tenant);
 if(statuses[response.status])statuses[response.status].add(1);
 if(response.status!==200){
  let code='UNPARSEABLE';let requestId=null;
  try{const error=response.json().error;code=error.code;requestId=error.request_id;}catch(_){}
  console.warn(JSON.stringify({event:'request_failure',time_ms:Date.now(),status:response.status,code,request_id:requestId,duration_ms:response.timings.duration}));
 }
 let valid=false;
 try{const body=response.json();valid=response.status===200 && (checks?body.results.length===100 && body.results.every(x=>x.decision===(allow?'allow':'deny')):body.decision===(allow?'allow':'deny'));}catch(_){valid=false;}
 if(valid){correct.add(1);successes.add(response.timings.duration);}else{unexpected.add(1);if(response.status===200){mismatches.add(1);}}
}
export function handleSummary(data){return {[__ENV.RESULT]:JSON.stringify(data,null,2)};}
