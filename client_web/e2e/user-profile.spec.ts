import { expect, test } from '@playwright/test';

import { authenticatedResponse, mockWebSocket, useEnglish, useSavedSession } from './support/mockBackend';

test('does not show a profile loading state when the viewer is logged out', async ({ page }) => {
  await useEnglish(page);
  await mockWebSocket(page, () => undefined);

  const consoleErrors: string[] = [];
  page.on('console', (message) => {
    if (message.type() === 'error') consoleErrors.push(message.text());
  });
  page.on('pageerror', (error) => consoleErrors.push(error.message));

  await page.goto('/user/alice');
  await expect(page.getByText('Please login first.')).toBeVisible();
  await expect(page.getByText('Loading profile...')).toHaveCount(0);
  expect(consoleErrors).toEqual([]);
});

test('shows passed problems and sorts them by number, first accepted time, and difficulty', async ({ page }) => {
  await useSavedSession(page);
  await mockWebSocket(page, (request) => {
    if (request.type === 'user_profile') {
      return {
        type: 'user_profile',
        content: {
          request_key: request.content.request_key,
          username: 'alice',
          accepted: 3,
          test_accepted: 0,
          general: 3,
          accepted_submission_timestamps: [1_500_000_000, 1_600_000_000, 1_700_000_000],
          accepted_problems: [
            { problem_number: 1002, problem_name: 'Second', difficulty: 2, first_accepted_at: 1_700_000_000 },
            { problem_number: 1000, problem_name: 'First', difficulty: 3, first_accepted_at: 1_600_000_000 },
            { problem_number: 1001, problem_name: 'Easy', difficulty: 1, first_accepted_at: 1_500_000_000 },
          ],
        },
      };
    }
    return authenticatedResponse(request);
  });

  await page.goto('/user/alice');
  const trend = page.getByRole('region', { name: 'Problem-solving trend (AC submissions)' });
  const trendCollapse = trend.getByRole('button', { name: 'Collapse problem-solving trend' });
  await expect(trendCollapse).toHaveAttribute('aria-expanded', 'true');
  await trendCollapse.click();
  await expect(trend.getByRole('combobox')).toHaveCount(0);

  const trendExpand = trend.getByRole('button', { name: 'Expand problem-solving trend' });
  await expect(trendExpand).toHaveAttribute('aria-expanded', 'false');
  await trendExpand.click();
  await expect(trend.getByRole('combobox', { name: 'Chart granularity' })).toBeVisible();

  const region = page.getByRole('region', { name: 'Passed problems' });
  const table = region.getByRole('table');
  await expect(table.getByRole('row').nth(1).getByRole('cell').nth(0)).toHaveText('1000');
  await expect(table.getByRole('row').nth(2).getByRole('cell').nth(0)).toHaveText('1001');
  await expect(table.getByRole('row').nth(3).getByRole('cell').nth(0)).toHaveText('1002');

  const sort = region.getByRole('combobox', { name: 'Sort passed problems' });
  await sort.click();
  await page.getByRole('option', { name: 'First accepted time' }).click();
  await expect(table.getByRole('row').nth(1).getByRole('cell').nth(0)).toHaveText('1001');
  await expect(table.getByRole('row').nth(2).getByRole('cell').nth(0)).toHaveText('1000');
  await expect(table.getByRole('row').nth(3).getByRole('cell').nth(0)).toHaveText('1002');

  await sort.click();
  await page.getByRole('option', { name: 'Difficulty' }).click();
  await expect(table.getByRole('row').nth(1).getByRole('cell').nth(0)).toHaveText('1001');
  await expect(table.getByRole('row').nth(2).getByRole('cell').nth(0)).toHaveText('1002');
  await expect(table.getByRole('row').nth(3).getByRole('cell').nth(0)).toHaveText('1000');

  const collapse = region.getByRole('button', { name: 'Collapse passed problems' });
  await expect(collapse).toHaveAttribute('aria-expanded', 'true');
  await collapse.click();
  await expect(region.getByRole('table')).toHaveCount(0);

  const expand = region.getByRole('button', { name: 'Expand passed problems' });
  await expect(expand).toHaveAttribute('aria-expanded', 'false');
  await expand.click();
  await expect(region.getByRole('table')).toBeVisible();
});
