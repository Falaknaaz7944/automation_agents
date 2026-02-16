import { chromium } from "playwright";
import path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// auth.json saved next to this file
const authPath = path.join(__dirname, "auth.json");

const text = process.argv.slice(2).join(" ").trim();
if (!text) {
  console.error("❌ No post text provided. Usage: linkedin post: <text>");
  process.exit(1);
}

(async () => {
  const browser = await chromium.launch({ headless: false });

  const context = await browser.newContext({
    storageState: authPath,
  });

  const page = await context.newPage();

  await page.goto("https://www.linkedin.com/feed/", { waitUntil: "domcontentloaded" });

  // if session is invalid
  if (page.url().includes("/login")) {
    console.error("❌ Session not valid. Run `linkedin login` again.");
    await browser.close();
    process.exit(1);
  }

  // ✅ Open post composer (LinkedIn labels vary)
  const startBtn = page.getByRole("button", { name: /start a post/i });
  await startBtn.waitFor({ state: "visible", timeout: 20000 });
  await startBtn.click();

  // ✅ Wait for the modal/dialog
  const dialog = page.locator("div[role='dialog']").first();
  await dialog.waitFor({ state: "visible", timeout: 20000 });

  // ✅ Find the editor inside the dialog (contenteditable)
  // LinkedIn changes DOM a lot, so we use a robust locator + fallback.
  let editor = dialog.locator('div[contenteditable="true"]').first();

  // fallback: sometimes it's a <p> inside a contenteditable wrapper
  if ((await editor.count()) === 0) {
    editor = dialog.locator('[contenteditable="true"]').first();
  }

  await editor.waitFor({ state: "visible", timeout: 20000 });

  // ✅ Focus editor + type (type is more reliable than fill on LinkedIn)
  await editor.click({ timeout: 10000 });
  await page.keyboard.down("Control");
  await page.keyboard.press("A");
  await page.keyboard.up("Control");
  await page.keyboard.press("Backspace");

  await page.keyboard.type(text, { delay: 10 });

  // ✅ Wait for Post button to become enabled, THEN click
  // The "Post" button can be disabled until text is detected.
  const postBtn = dialog.getByRole("button", { name: /^post$/i });

  await postBtn.waitFor({ state: "visible", timeout: 20000 });

  // wait until enabled
  await page.waitForFunction(
    (btn) => btn && !btn.disabled && btn.getAttribute("aria-disabled") !== "true",
    await postBtn.elementHandle(),
    { timeout: 20000 }
  );

  await postBtn.click();

  // ✅ optional: wait so you can see it posted
  await page.waitForTimeout(3000);

  console.log("✅ Posted to LinkedIn successfully.");
  await browser.close();
})();
