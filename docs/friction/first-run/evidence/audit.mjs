import { chromium } from 'playwright';
import { writeFileSync } from 'node:fs';
const browser = await chromium.launch();
const context = await browser.newContext({ignoreHTTPSErrors:true});
const page = await context.newPage();
const evidence='/private/tmp/kestrel-friction-488/evidence';
const errors=[]; page.on('pageerror', e=>errors.push(e.message));
async function snapshot(name){writeFileSync(`${evidence}/${name}.txt`,await page.locator('body').innerText());await page.screenshot({path:`${evidence}/${name}.png`,fullPage:true});}
await page.goto('https://127.0.0.1:17819');
await page.getByText('No Organization exists yet.',{exact:false}).waitFor();
await snapshot('browser-fresh');
await context.route('**/operator/**',route=>route.abort());
await page.reload();await page.waitForTimeout(4000);await snapshot('browser-offline');
await context.unroute('**/operator/**');
await context.route('**/operator/organizations', async route=>{await new Promise(r=>setTimeout(r,2500));await route.continue();});
await page.reload({waitUntil:'domcontentloaded'});await snapshot('browser-loading');
writeFileSync(`${evidence}/browser-errors.json`,JSON.stringify(errors,null,2));
await browser.close();
