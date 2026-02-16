import { chromium } from "playwright";
import path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const authPath = path.join(__dirname, "auth.json");

const commentText = process.argv.slice(2).join(" ").trim();
if (!commentText) {
  console.error("❌ No comment text provided.");
  process.exit(1);
}

(async () => {
  const browser = await chromium.launch({ headless: false });

  const context = await browser.newContext({ storageState: authPath });
  const page = await context.newPage();

  await page.goto("https://www.linkedin.com/search/results/content/?keywords=%23openclaw");
  await page.waitForLoadState("domcontentloaded");

  if (page.url().includes("/login")) {
    console.error("❌ Session invalid. Run `linkedin login` again.");
    await browser.close();
    process.exit(1);
  }

  // Scroll a bit so posts load
  await page.mouse.wheel(0, 1200);
  await page.waitForTimeout(1500);

  // Try to comment on first 1-2 visible posts
  const commentButtons = page.getByRole("button", { name: /comment/i });
  const count = await commentButtons.count();

  if (count === 0) {
    console.log("⚠️ No comment buttons found.");
    await browser.close();
    return;
  }

  const max = Math.min(2, count);
  for (let i = 0; i < max; i++) {
    await commentButtons.nth(i).click();
    await page.waitForTimeout(800);

    // Comment box in LinkedIn is contenteditable
    const editor = page.locator("[contenteditable='true']").last();
    await editor.click();
    await editor.fill(commentText);

    // Click "Post" for comment
    const postBtn = page.getByRole("button", { name: /^post$/i }).last();
    await postBtn.click();

    await page.waitForTimeout(2000);
  }

  console.log(`✅ Commented on ${max} post(s) under #openclaw`);
  await browser.close();
})();
