import { chromium } from "playwright";
import path from "path";
import fs from "fs";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const authPath = path.join(__dirname, "auth.json");

(async () => {
  const browser = await chromium.launch({ headless: false });
  const context = await browser.newContext();

  const page = await context.newPage();
  await page.goto("https://www.linkedin.com/login");
  console.log("✅ Please login in the opened browser window...");

  // Wait until feed loads after login
  await page.waitForURL(/linkedin\.com\/feed/, { timeout: 180000 });

  // ✅ ensure folder exists
  fs.mkdirSync(__dirname, { recursive: true });

  // ✅ save session
  await context.storageState({ path: authPath });

  console.log("✅ LinkedIn session saved to auth.json");
  await browser.close();
})();
