import { test, expect } from "@playwright/test";
for (const scale of [1, 1.25, 1.5])
  for (const width of [1180, 900]) {
    test(`aligned table and tabs at ${scale * 100}% / ${width}`, async ({
      browser,
    }) => {
      const context = await browser.newContext({
        viewport: { width, height: 850 },
        deviceScaleFactor: scale,
      });
      const page = await context.newPage();
      await page.goto("/?fixtures");
      await expect(
        page.getByText("Research manuscript", { exact: true }),
      ).toBeVisible();
      // CSS zoom additionally exercises layout reflow, not only raster DPI.
      await page.evaluate((s) => {
        document.documentElement.style.zoom = String(s);
      }, scale);
      const table = page.locator(".project-table");
      const offsets = await table.evaluate((t) => {
        const h = [...t.querySelectorAll("th")],
          d = [...t.querySelectorAll("tbody tr:first-child td")];
        return h.map((e, i) => ({
          header: e.getBoundingClientRect().left,
          cell: d[i].getBoundingClientRect().left,
          ha: getComputedStyle(e).textAlign,
          da: getComputedStyle(d[i]).textAlign,
        }));
      });
      for (const o of offsets) {
        expect(Math.abs(o.header - o.cell)).toBeLessThan(1);
        expect(o.ha).toBe(o.da);
      }
      expect(
        await page.evaluate(
          () => document.documentElement.scrollWidth <= innerWidth,
        ),
      ).toBe(true);
      await page.getByRole("button", { name: "全局设置", exact: true }).click();
      const tabs = page.getByRole("tab");
      let boxes = await tabs.evaluateAll((es) =>
        es.map((e) => {
          const r = e.getBoundingClientRect();
          return { x: r.x, y: r.y, h: r.height, w: r.width };
        }),
      );
      expect(boxes[0].h).toBe(boxes[1].h);
      expect(boxes[0].y).toBe(boxes[1].y);
      await page.getByRole("tab", { name: "GitHub" }).click();
      const after = await tabs.evaluateAll((es) =>
        es.map((e) => {
          const r = e.getBoundingClientRect();
          return { y: r.y, h: r.height };
        }),
      );
      expect(after[0]).toEqual(after[1]);
      expect(after[0].h).toBe(boxes[0].h);
      await page.getByRole("tab", { name: "Overleaf" }).click();
      const tokens = page.locator(".token-table");
      await expect(tokens).toBeVisible();
      const aligned = await tokens.evaluate((t) => {
        const h = [...t.querySelectorAll("th")],
          d = [...t.querySelectorAll("tbody tr:first-child td")];
        return h.every(
          (e, i) =>
            Math.abs(
              e.getBoundingClientRect().left -
                d[i].getBoundingClientRect().left,
            ) < 1,
        );
      });
      expect(aligned).toBe(true);
      await page.screenshot({
        path: `test-results/settings-${width}-${scale}.png`,
        fullPage: true,
      });
      await context.close();
    });
  }
test("empty state, add edit pause delete project", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("从第一个同步项目开始")).toBeVisible();
  await page.getByRole("button", { name: "新增同步", exact: true }).click();
  await page.getByRole("button", { name: "浏览", exact: true }).click();
  await page.getByLabel("项目名称", { exact: true }).fill("Example project");
  await page.getByRole("button", { name: "创建同步", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await page
    .getByRole("button", { name: "暂停 Example project", exact: true })
    .click();
  await expect(page.locator(".project-table .status")).toHaveText("已暂停");
  await page
    .getByRole("button", { name: "Example project", exact: true })
    .click();
  await page.getByRole("button", { name: "编辑项目", exact: true }).click();
  await page.getByLabel("项目名称", { exact: true }).fill("Renamed project");
  await page.getByRole("button", { name: "保存修改", exact: true }).click();
  await expect(page.locator(".project-table")).toContainText("Renamed project");
  await page.getByRole("button", { name: "删除项目", exact: true }).click();
  await page.getByRole("button", { name: "确认删除", exact: true }).click();
  await expect(page.getByText("从第一个同步项目开始")).toBeVisible();
});
test("token add, mask, default and delete", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "全局设置", exact: true }).click();
  await page.getByRole("tab", { name: "GitHub" }).click();
  await page.getByRole("button", { name: "新增 Token", exact: true }).click();
  await page.getByLabel("名称", { exact: true }).fill("Personal");
  await page.getByLabel("Token", { exact: true }).fill("ghp_dummy_secret_1234");
  await page.getByRole("button", { name: "添加 Token", exact: true }).click();
  await expect(page.locator(".token-mask")).toHaveText("ghp_********1234");
  await expect(page.locator("body")).not.toContainText("ghp_dummy_secret_1234");
  await expect(page.locator(".default-badge")).toHaveText("默认");
  await page
    .getByRole("button", { name: "删除 Token Personal", exact: true })
    .click();
  await page.getByRole("button", { name: "确认删除", exact: true }).click();
  await expect(page.getByText("还没有添加 GitHub Token")).toBeVisible();
});
test("screenshots and dialog stays on screen", async ({ page }) => {
  await page.goto("/?fixtures");
  await expect(
    page.getByText("Research manuscript", { exact: true }),
  ).toBeVisible();
  await page.screenshot({ path: "test-results/projects.png", fullPage: true });
  await page
    .getByRole("button", { name: "Research manuscript", exact: true })
    .click();
  await page.screenshot({
    path: "test-results/project-details.png",
    fullPage: true,
  });
  await page.getByRole("button", { name: "新增同步", exact: true }).click();
  await page.screenshot({
    path: "test-results/add-project.png",
    fullPage: true,
  });
  const box = await page.getByRole("dialog").boundingBox();
  expect(box!.y).toBeGreaterThanOrEqual(0);
  expect(box!.y + box!.height).toBeLessThanOrEqual(800);
});

test("nonempty directory requires confirmation and cancel keeps the form", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "新增同步", exact: true }).click();
  await page.getByRole("tab", { name: "GitHub" }).click();
  await page.getByLabel("项目名称", { exact: true }).fill("Existing files");
  await page
    .getByPlaceholder("选择文件夹或输入新目录的完整路径")
    .fill("D:\\new-folder-nonempty");
  await page
    .getByLabel("远程仓库地址", { exact: true })
    .fill("https://github.com/example/project.git");
  await page.getByRole("button", { name: "创建同步", exact: true }).click();
  await expect(
    page.getByRole("heading", { name: "使用这个非空目录？" }),
  ).toBeVisible();
  await expect(page.locator(".project-table")).toHaveCount(0);
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await expect(page.getByLabel("项目名称", { exact: true })).toHaveValue(
    "Existing files",
  );
  await page.getByRole("button", { name: "创建同步", exact: true }).click();
  await page.getByRole("button", { name: "继续创建", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.locator(".project-table")).toContainText("Existing files");
});
test("new empty directory proceeds without the nonempty warning", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "新增同步", exact: true }).click();
  await page.getByRole("tab", { name: "GitHub" }).click();
  await page.getByLabel("项目名称", { exact: true }).fill("New folder");
  await page
    .getByPlaceholder("选择文件夹或输入新目录的完整路径")
    .fill("D:\\new-folder-empty");
  await page
    .getByLabel("远程仓库地址", { exact: true })
    .fill("https://github.com/example/project.git");
  await page.getByRole("button", { name: "创建同步", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await expect(page.locator(".project-table")).toContainText("New folder");
});

test("bulk selection, partial selection, cancel and delete", async ({
  page,
}) => {
  await page.goto("/?fixtures");
  const all = page.getByRole("checkbox", { name: "全选当前列表" });
  const start = page.getByRole("button", { name: "启动", exact: true });
  await expect(start).toBeDisabled();
  await page
    .getByRole("checkbox", { name: "选择 Research manuscript", exact: true })
    .check();
  await expect(page.locator(".selection-count")).toHaveText("已选 1 项");
  expect(await all.evaluate((e: HTMLInputElement) => e.indeterminate)).toBe(
    true,
  );
  await expect(page.locator(".detail")).toHaveCount(0);
  await page.getByRole("button", { name: "暂停", exact: true }).click();
  await expect(page.locator(".project-table .status")).toHaveText([
    "已暂停",
    "已暂停",
  ]);
  await all.check();
  await start.click();
  await expect(page.locator(".project-table .status")).toHaveText([
    "已同步",
    "已同步",
  ]);
  await all.check();
  await page.getByRole("button", { name: "删除", exact: true }).click();
  await expect(page.getByRole("dialog")).toContainText(
    "本地文件和远程仓库会保留",
  );
  await page.getByRole("button", { name: "取消", exact: true }).click();
  await expect(page.locator(".project-table tbody tr")).toHaveCount(2);
  await page.getByRole("button", { name: "删除", exact: true }).click();
  await page.getByRole("button", { name: "确认删除", exact: true }).click();
  await expect(page.getByText("从第一个同步项目开始")).toBeVisible();
});

test("filtered select all is local, global pause and resume cover hidden projects", async ({
  page,
}) => {
  await page.goto("/?fixtures");
  await page.getByRole("checkbox", { name: "全选当前列表" }).check();
  await page.getByRole("textbox", { name: "搜索项目" }).fill("homepage");
  await expect(page.locator(".selection-count")).toHaveText("已选 0 项");
  await page.getByRole("checkbox", { name: "全选当前列表" }).check();
  await expect(page.locator(".selection-count")).toHaveText("已选 1 项");
  await page.getByRole("button", { name: "全部暂停", exact: true }).click();
  await page.getByRole("button", { name: "清空搜索" }).click();
  await expect(page.locator(".project-table .status")).toHaveText([
    "已暂停",
    "已暂停",
  ]);
  await page.getByRole("textbox", { name: "搜索项目" }).fill("homepage");
  await page.getByRole("button", { name: "全部恢复", exact: true }).click();
  await page.getByRole("button", { name: "清空搜索" }).click();
  await expect(page.locator(".project-table .status")).toHaveText([
    "已同步",
    "已同步",
  ]);
});

test("new projects default to one month, term bounds and permanent are editable", async ({
  page,
}) => {
  await page.goto("/");
  await page.getByRole("button", { name: "新增同步", exact: true }).click();
  await page.getByRole("button", { name: "浏览", exact: true }).click();
  await expect(page.getByLabel("同步期限", { exact: true })).toHaveValue(
    "months",
  );
  await expect(page.getByLabel("月数", { exact: true })).toHaveValue("1");
  await page.getByLabel("月数", { exact: true }).fill("13");
  await page.getByRole("button", { name: "创建同步", exact: true }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.getByLabel("同步期限", { exact: true }).selectOption("days");
  await expect(page.getByLabel("天数", { exact: true })).toHaveAttribute(
    "max",
    "30",
  );
  await page.getByLabel("天数", { exact: true }).fill("0");
  await page.getByRole("button", { name: "创建同步", exact: true }).click();
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.getByLabel("天数", { exact: true }).fill("30");
  await page.getByLabel("项目名称", { exact: true }).fill("Deadline example");
  await page.getByRole("button", { name: "创建同步", exact: true }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);
  await page
    .getByRole("button", { name: "Deadline example", exact: true })
    .click();
  const deadline = await page.locator(".detail dd").last().textContent();
  await page.getByRole("button", { name: "编辑项目", exact: true }).click();
  await expect(page.getByLabel("期限设置", { exact: true })).toHaveValue(
    "keep",
  );
  await expect(page.getByLabel("天数", { exact: true })).toHaveCount(0);
  await page.getByLabel("期限设置", { exact: true }).selectOption("reset");
  await page.getByLabel("天数", { exact: true }).fill("3");
  await expect(
    page.getByText("保存后再同步 3 天。到期自动暂停，项目和文件会保留。", {
      exact: true,
    }),
  ).toBeVisible();
  await page.getByLabel("期限设置", { exact: true }).selectOption("keep");
  await page.getByLabel("项目名称", { exact: true }).fill("Renamed deadline");
  await page.getByRole("button", { name: "保存修改", exact: true }).click();
  await expect(page.locator(".detail dd").last()).toHaveText(deadline!);
  await page.getByRole("button", { name: "编辑项目", exact: true }).click();
  await page.getByLabel("期限设置", { exact: true }).selectOption("reset");
  await page.getByLabel("同步期限", { exact: true }).selectOption("permanent");
  await expect(page.getByLabel("天数", { exact: true })).toHaveCount(0);
  await page.getByRole("button", { name: "保存修改", exact: true }).click();
  await expect(page.locator(".detail dd").last()).toHaveText("永久");
});

test("expired project is retained, skipped by resume all and can be renewed", async ({
  page,
}) => {
  await page.clock.install({ time: new Date("2026-09-24T12:00:00Z") });
  await page.goto("/?fixtures&expiry");
  await expect(
    page.getByRole("button", { name: "Research manuscript", exact: true }),
  ).toBeVisible();
  await page.clock.fastForward(4000);
  await expect(page.locator(".project-table .status").first()).toHaveText(
    "已到期",
  );
  await expect(
    page.getByRole("button", {
      name: "立即同步 Research manuscript",
      exact: true,
    }),
  ).toBeDisabled();
  await page.getByRole("button", { name: "全部恢复", exact: true }).click();
  await expect(page.getByRole("alert")).toContainText("期限已到");
  await expect(page.locator(".project-table .status").first()).toHaveText(
    "已到期",
  );
  await expect(page.locator(".project-table tbody tr")).toHaveCount(2);
  await page
    .getByRole("button", { name: "续期 Research manuscript", exact: true })
    .click();
  await page.getByLabel("期限设置", { exact: true }).selectOption("reset");
  await expect(
    page.getByRole("checkbox", { name: "保存后启用自动同步", exact: true }),
  ).toBeChecked();
  await page.screenshot({
    path: "test-results/renew-term.png",
    fullPage: true,
  });
  await page.getByRole("button", { name: "保存修改", exact: true }).click();
  await expect(page.locator(".project-table .status").first()).toHaveText(
    "等待同步",
  );
  await expect(
    page.getByRole("button", {
      name: "立即同步 Research manuscript",
      exact: true,
    }),
  ).toBeEnabled();
});

test("projects sort by name and interval columns", async ({ page }) => {
  await page.goto("/?fixtures");
  const first = page.locator(".project-table .project-name").first();
  await expect(first).toHaveText("Research manuscript");
  await page.getByRole("button", { name: "按项目排序" }).click();
  await expect(first).toHaveText("Project homepage");
  await page.getByRole("button", { name: "按项目排序" }).click();
  await expect(first).toHaveText("Research manuscript");
  await page.getByRole("button", { name: "按同步间隔排序" }).click();
  await expect(first).toHaveText("Research manuscript");
  await page.getByRole("button", { name: "按同步间隔排序" }).click();
  await expect(first).toHaveText("Project homepage");
});

test("theme switcher applies light and dark themes", async ({ page }) => {
  await page.goto("/");
  await page.getByRole("button", { name: "全局设置", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.getByRole("button", { name: "深色", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
  await page.getByRole("button", { name: "浅色", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  await page.getByRole("button", { name: "跟随系统", exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
});
