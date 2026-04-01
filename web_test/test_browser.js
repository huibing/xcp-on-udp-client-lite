const { chromium } = require('playwright');
const path = require('path');

(async () => {
  const browser = await chromium.launch({ headless: false });
  const page = await browser.newPage();

  console.log('=== Step 0: Logout first ===');
  await page.goto('http://localhost:8080/logout').catch(() => {});
  await page.waitForTimeout(500);

  console.log('\n=== Step 1: Login ===');
  await page.goto('http://localhost:8080/login');
  await page.fill('input[name="username"]', 'admin');
  await page.fill('input[name="password"]', 'admin');
  await page.click('button[type="submit"]');
  await page.waitForSelector('a[href="/calibration"]', { timeout: 5000 });
  console.log('Logged in successfully');

  console.log('\n=== Step 2: Check Dashboard - no slaves connected ===');
  await page.goto('http://localhost:8080/');
  await page.waitForTimeout(1000);
  await page.screenshot({ path: 'screenshot_dashboard_initial.png' });

  console.log('\n=== Step 3: Add a new slave ===');
  // Look for "Add Device" or similar button
  const addDeviceBtn = await page.$('button:has-text("Add")');
  if (addDeviceBtn) {
    await addDeviceBtn.click();
    await page.waitForTimeout(500);

    // Fill in slave details (127.0.0.1:5555)
    const hostInput = await page.$('input[name="host"]');
    if (hostInput) {
      await hostInput.fill('127.0.0.1');
    }

    const portInput = await page.$('input[name="port"]');
    if (portInput) {
      await portInput.fill('5555');
    }

    // Click connect/submit
    const submitBtn = await page.$('button[type="submit"]');
    if (submitBtn) {
      await submitBtn.click();
      await page.waitForTimeout(2000);
    }
  } else {
    console.log('No "Add Device" button found, trying direct connect form');

    // Try finding a form with host/port inputs
    const hostInput = await page.$('input[placeholder*="host" i], input[name="host"]');
    if (hostInput) {
      await hostInput.fill('127.0.0.1');
      const portInput = await page.$('input[placeholder*="port" i], input[name="port"]');
      if (portInput) {
        await portInput.fill('5555');
      }
      // Find connect button
      const connectBtn = await page.$('button:has-text("Connect")');
      if (connectBtn) {
        await connectBtn.click();
        await page.waitForTimeout(2000);
      }
    }
  }

  await page.screenshot({ path: 'screenshot_after_connect.png' });
  console.log('Screenshot: screenshot_after_connect.png');

  console.log('\n=== Step 4: Go to A2L page ===');
  await page.click('a[href="/a2l"]');
  await page.waitForTimeout(1000);
  await page.screenshot({ path: 'screenshot_a2l_page.png' });

  console.log('\n=== Step 5: Upload abc.a2l ===');
  const a2lPath = path.resolve('abc.a2l');
  console.log('A2L file path:', a2lPath);

  const fileInput = await page.locator('input[type="file"]');
  await fileInput.setInputFiles(a2lPath);
  await page.waitForTimeout(3000);
  await page.screenshot({ path: 'screenshot_after_upload.png' });

  console.log('\n=== Step 6: Go to Calibration page ===');
  await page.click('a[href="/calibration"]');
  await page.waitForTimeout(2000);
  await page.screenshot({ path: 'screenshot_calibration.png', fullPage: true });

  // Get calibration parameters
  try {
    const paramNames = await page.$$eval('table tbody tr td:first-child', cells =>
      cells.map(c => c.textContent.trim()).filter(Boolean)
    );
    console.log('\nCalibration parameters found:', paramNames);
  } catch (e) {
    console.log('Could not find calibration table');
  }

  console.log('\n=== Step 7: Check Dashboard again ===');
  await page.click('a[href="/"]');
  await page.waitForTimeout(1000);
  await page.screenshot({ path: 'screenshot_dashboard_final.png' });

  console.log('\nTest completed. Browser will close in 5 seconds...');
  await page.waitForTimeout(5000);

  await browser.close();
})().catch(console.error);
