import { chromium } from "playwright";
import path from "path";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// session file next to scripts
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

  // Go directly to hashtag results
  await page.goto(
    "https://www.linkedin.com/search/results/content/?keywords=%23openclaw&origin=GLOBAL_SEARCH_HEADER",
    { waitUntil: "domcontentloaded" }
  );

  // If session is invalid, LinkedIn will redirect to login
  if (page.url().includes("/login")) {
    console.error("❌ Session not valid. Run `linkedin login` again.");
    await browser.close();
    process.exit(1);
  }

  // Open first result and comment (simple + demo-friendly)
  // Click first result card (LinkedIn UI changes often, so we keep it robust)
  await page.waitForTimeout(2000);

  // Try to click the first visible "Comment" button
  const commentBtn = page.getByRole("button", { name: /comment/i }).first();
  await commentBtn.click();

  // Wait for comment box and type
  await page.waitForTimeout(1200);

  // This grabs the active textbox after opening comment
  const box = page.getByRole("textbox").last();
  await box.click();
  await box.fill(commentText);

  // Click "Post" button for comment
  const postBtn = page.getByRole("button", { name: /^post$/i }).first();
  await postBtn.click();

  console.log("✅ Comment posted on #openclaw content.");
  await page.waitForTimeout(1500);
  await browser.close();
})();
