import { chromium } from "playwright";
import path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const authPath = path.join(__dirname, "auth.json");

// usage: node linkedin_comment.js "your comment text"
const commentText = process.argv.slice(2).join(" ").trim();
if (!commentText) {
  console.error("❌ No comment provided. Usage: linkedin_comment.js <text>");
  process.exit(1);
}

async function safeClick(locator, timeout = 15000) {
  await locator.first().waitFor({ state: "visible", timeout });
  await locator.first().click({ timeout });
}

(async () => {
  const browser = await chromium.launch({ headless: false });
  const context = await browser.newContext({ storageState: authPath });
  const page = await context.newPage();

  // 1) Go to LinkedIn content search for #openclaw
  const url =
    "https://www.linkedin.com/search/results/content/?keywords=%23openclaw&origin=SWITCH_SEARCH_VERTICAL";
  await page.goto(url, { waitUntil: "domcontentloaded" });

  // If session invalid
  if (page.url().includes("/login")) {
    console.error("❌ Session not valid. Run `linkedin login` again.");
    await browser.close();
    process.exit(1);
  }

  // 2) Wait for results to render
  await page.waitForTimeout(2000);
  await page.waitForLoadState("networkidle").catch(() => {});

  // 3) Find the first visible post container
  // LinkedIn uses "feed-shared-update-v2" a lot for content posts
  const firstPost = page.locator('[data-urn^="urn:li:activity"]').first();

  await firstPost.waitFor({ state: "visible", timeout: 20000 });

  // 4) Click the Comment button/icon in that post
  // Common labels: "Comment", "Comment on", "Comment button"
  const commentBtn = firstPost
    .getByRole("button", { name: /comment/i })
    .or(firstPost.locator('button[aria-label*="Comment"]'));

  await safeClick(commentBtn, 20000);

  // 5) Now the comment editor should appear near that post
  // Editor is usually contenteditable=true inside a form
  const editor = firstPost
    .locator('div[contenteditable="true"]')
    .first();

  await editor.waitFor({ state: "visible", timeout: 20000 });

  // Focus + type
  await editor.click();
  // Fill sometimes fails on contenteditable depending on LinkedIn
  // so we use type
  await editor.type(commentText, { delay: 5 });

  // 6) Find a submit button (varies: Post / Reply / Comment)
  // IMPORTANT: often it becomes enabled only after typing
  const submitBtn = firstPost
    .getByRole("button", { name: /^(post|reply|comment)$/i })
    .or(firstPost.locator('button[type="submit"]'))
    .or(firstPost.getByRole("button", { name: /post|reply/i }));

  await submitBtn.first().waitFor({ state: "visible", timeout: 20000 });

  // Wait until enabled (not disabled)
  await page.waitForTimeout(500);
  const isDisabled = await submitBtn.first().getAttribute("disabled");
  if (isDisabled !== null) {
    // sometimes LinkedIn needs another click in editor
    await editor.click();
    await page.waitForTimeout(500);
  }

  await submitBtn.first().click({ timeout: 20000 });

  // 7) Small wait so you can see it happened
  await page.waitForTimeout(2000);

  console.log("✅ Comment posted successfully on #openclaw content.");
  await browser.close();
})();
