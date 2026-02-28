/**
 * WebdriverIO helpers for UNFUDGED E2E tests.
 */

/** Wait for an element to be visible */
export async function waitFor(selector: string, timeout = 10000): Promise<WebdriverIO.Element> {
  const el = await $(selector);
  await browser.waitUntil(() => el.isDisplayed(), {
    timeout,
    timeoutMsg: `"${selector}" not visible after ${timeout}ms`
  });
  return el;
}

/** Wait for text to appear anywhere on the page */
export async function waitForText(text: string, timeout = 10000): Promise<void> {
  await browser.waitUntil(
    async () => (await browser.getPageSource()).includes(text),
    { timeout, timeoutMsg: `Text "${text}" not found after ${timeout}ms` }
  );
}

/** Get text content from an element */
export async function getText(selector: string): Promise<string> {
  const el = await $(selector);
  return el.getText();
}

/** Click an element and wait for UI to settle */
export async function click(selector: string, settleMs = 500): Promise<void> {
  const el = await $(selector);
  await el.click();
  await browser.pause(settleMs);
}

/** Type into an input, clearing first */
export async function type(selector: string, text: string): Promise<void> {
  const el = await $(selector);
  await el.clearValue();
  await el.setValue(text);
}

/** Take a screenshot (saved to ./screenshots/) */
export async function screenshot(name: string): Promise<void> {
  await browser.saveScreenshot(`./screenshots/${name}.png`);
}
