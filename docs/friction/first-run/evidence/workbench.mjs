import { chromium } from 'playwright';
import {writeFileSync} from 'node:fs';
const browser=await chromium.launch();const context=await browser.newContext({ignoreHTTPSErrors:true,viewport:{width:1280,height:900}});const page=await context.newPage();const out='/private/tmp/kestrel-friction-488/evidence';
async function snap(name){writeFileSync(`${out}/${name}.txt`,await page.locator('body').innerText());await page.screenshot({path:`${out}/${name}.png`,fullPage:true});}
await page.goto('https://127.0.0.1:17819/organizations/audit/new');await page.getByLabel('Your name',{exact:true}).waitFor();await snap('browser-new');
await page.getByLabel('Your name',{exact:true}).fill('audit-operator');await page.getByLabel('Brief',{exact:true}).fill('Reply with audit complete. Do not edit files.');await page.getByRole('button',{name:'Open Workspace',exact:true}).click();await page.waitForURL('**/workspaces/**');await page.waitForTimeout(2500);await snap('browser-session');
for(const tab of ['Work','Diff','Files','Sessions','People']){await page.getByRole('tab',{name:tab,exact:true}).click();await page.waitForTimeout(750);await snap('browser-tab-'+tab.toLowerCase());}
await page.setViewportSize({width:375,height:812});await snap('browser-mobile');
await context.route('**/operator/organizations/audit/projects',route=>route.fulfill({status:503,contentType:'application/json',body:JSON.stringify({message:'audit projects unavailable'})}));
await page.goto('https://127.0.0.1:17819/organizations/audit/new');await page.waitForTimeout(4500);await snap('browser-projects-error');
await context.unroute('**/operator/organizations/audit/projects');
await page.setViewportSize({width:1280,height:900});await page.goto('https://127.0.0.1:17819/organizations/audit/workspaces/missing');await page.waitForTimeout(2500);await snap('browser-missing-workspace');
await browser.close();
