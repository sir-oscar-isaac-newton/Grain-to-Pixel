// TEMPORARY (T-07 spike): attaches msedgedriver to the already open debugging port and reads the
// page title, proving the whole chain works on the runner.
import path from 'node:path';
import { createRequire } from 'node:module';

const root = process.argv[2];
const require = createRequire(path.join(root, 'apps/desktop/package.json'));
const { Builder } = require('selenium-webdriver');
const edge = require('selenium-webdriver/edge');

const driverPath = path.join(root, '.cache', 'edgedriver', 'msedgedriver.exe');
const options = new edge.Options();
options.debuggerAddress('127.0.0.1:9222');
const driver = await new Builder()
  .forBrowser('MicrosoftEdge')
  .setEdgeOptions(options)
  .setEdgeService(new edge.ServiceBuilder(driverPath))
  .build();
try {
  console.log(`ATTACHED: title="${await driver.getTitle()}"`);
  const h2 = await driver.findElement({ css: 'h2' });
  console.log(`ATTACHED: first heading="${await h2.getText()}"`);
} finally {
  await driver.quit();
}
