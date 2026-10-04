import {chromium} from 'playwright';import{writeFileSync}from'node:fs';
const browser=await chromium.launch();const out='/private/tmp/kestrel-friction-488/evidence';
async function scenario(name,url,pattern,body,status=200,tab){const c=await browser.newContext({ignoreHTTPSErrors:true,viewport:{width:1280,height:900}});let hits=0;await c.route(pattern,r=>{hits++;return r.fulfill({status,contentType:'application/json',body:JSON.stringify(body)});});const p=await c.newPage();await p.goto('https://127.0.0.1:17819'+url);if(tab)await p.getByRole('tab',{name:tab,exact:true}).click();await p.waitForTimeout(name.includes("error") ? 10000 : 2200);writeFileSync(`${out}/${name}.txt`,await p.locator('body').innerText()+`\nInjected requests: ${hits}`);await p.screenshot({path:`${out}/${name}.png`,fullPage:true});await c.close();}
const ws='/organizations/audit/workspaces/rapid-thistle-vrsaqobm';
await scenario('browser-empty-projects','/organizations/audit/new','**/operator/organizations/audit/projects',[]);
await scenario('browser-empty-agents','/organizations/audit/new','**/operator/organizations/audit/agents',[]);
await scenario('browser-agents-error','/organizations/audit/new','**/operator/organizations/audit/agents',{message:'audit agents unavailable'},503);
await scenario('browser-profiles-error','/organizations/audit/new','**/operator/organizations/audit/profiles',{message:'audit profiles unavailable'},503);
await scenario('browser-queue-error','/organizations/audit/new','**/operator/organizations/audit/queue',{message:'audit queue unavailable'},503);
await scenario('browser-empty-workspaces','/organizations/audit','**/operator/organizations/audit/workspaces',[]);
await scenario('browser-work-error',ws,'**/operator/organizations/audit/workspaces/*/work',{message:'audit work unavailable'},503,'Work');
await scenario('browser-sessions-error',ws,'**/operator/organizations/audit/workspaces/*/sessions',{message:'audit sessions unavailable'},503,'Sessions');
await scenario('browser-transcript-error',ws,'**/operator/organizations/audit/workspaces/*/transcript?*',{message:'audit transcript unavailable'},503);
await browser.close();
