import { expect, test } from '@playwright/test';
import {
  authenticatedResponse,
  mockWebSocket,
  useSavedSession,
  type WebSocketRequest,
} from './support/mockBackend';

test.beforeEach(async ({ page }) => {
  await useSavedSession(page);
});

test('searches and paginates the problem list returned through WebSocket', async ({ page }) => {
  const problems = Array.from({ length: 12 }, (_, index) => ({
    problem_number: index + 1,
    problem_name: index === 6 ? 'Binary Search' : `Problem ${index + 1}`,
  }));
  await mockWebSocket(page, (request) => {
    if (request.type === 'problem_set') {
      return {
        type: 'problem_set',
        content: { request_key: request.content.request_key, problems },
      };
    }
    return authenticatedResponse(request);
  });

  await page.goto('/problem');
  await expect(page.getByRole('cell', { name: 'Problem 1', exact: true })).toBeVisible();

  await page.getByPlaceholder('Search by number or name').fill('binary');
  await expect(page.getByRole('cell', { name: 'Binary Search' })).toBeVisible();
  await expect(page.getByRole('cell', { name: 'Problem 1', exact: true })).toBeHidden();

  await page.getByPlaceholder('Search by number or name').clear();
  await page.getByRole('button', { name: 'Next' }).click();
  await expect(page.getByText('Page 2 / 2')).toBeVisible();
  await expect(page.getByRole('cell', { name: 'Problem 11', exact: true })).toBeVisible();
});

test('keeps the submissions list visible while opening a selected detail', async ({ page }) => {
  const requests: WebSocketRequest[] = [];
  await mockWebSocket(page, (request) => {
    if (request.type === 'total_submissions_list_index') {
      return {
        type: 'total_submissions_list_index',
        content: { request_key: request.content.request_key, total_submissions_list_index: 1 },
      };
    }
    if (request.type === 'submissions_list') {
      return {
        type: 'submissions_list',
        content: {
          request_key: request.content.request_key,
          submissions_list: [
            { submission_id: 101, username: 'alice', problem_number: 7, result: 'AC', general_score: 100, statuses: ['AC'], scores: [100] },
            { submission_id: 100, username: 'bob', problem_number: 3, result: 'WA' },
          ],
        },
      };
    }
    if (request.type === 'submission_result') {
      return {
        type: 'submission_result',
        content: {
          request_key: request.content.request_key,
          submission_id: 101,
          problem_number: 7,
          result: 'AC',
          general_score: 100,
          statuses: ['AC'],
          scores: [100],
          code: ['int main() { return 0; }'],
          language: 'cpp',
          username: 'alice',
        },
      };
    }
    return authenticatedResponse(request);
  }, requests);

  await page.goto('/submission');
  await expect(page.getByText('Select a submission to view its details.')).toBeVisible();
  await expect(page.getByLabel('My submission')).toBeVisible();

  await page.getByRole('cell', { name: /101/ }).click();
  await expect(page).toHaveURL(/\/submission\/101$/);
  await expect(page.getByText('Submission ID: 101')).toBeVisible();
  await expect(page.getByText('int main() { return 0; }')).toBeVisible();
  await expect(page.getByRole('cell', { name: /101/ })).toBeVisible();
  await expect(page.getByRole('cell', { name: 'alice', exact: true })).toBeVisible();
  await expect(page.getByRole('cell', { name: 'bob', exact: true })).toBeVisible();
  expect(requests.some((request) => request.type === 'submission_result')).toBe(true);
});

test('filters submissions by username and resets to the first page', async ({ page }) => {
  const requests: WebSocketRequest[] = [];
  await mockWebSocket(page, (request) => {
    if (request.type === 'total_submissions_list_index') {
      return {
        type: 'total_submissions_list_index',
        content: {
          request_key: request.content.request_key,
          total_submissions_list_index: request.content.username === 'bob' ? 1 : 2,
        },
      };
    }
    if (request.type === 'submissions_list') {
      const username = request.content.username as string;
      return {
        type: 'submissions_list',
        content: {
          request_key: request.content.request_key,
          submissions_list: username === 'bob'
            ? [{ submission_id: 200, username: 'bob', problem_number: 9, result: 'AC' }]
            : [{ submission_id: 100, username: 'alice', problem_number: 3, result: 'WA' }],
        },
      };
    }
    return authenticatedResponse(request);
  }, requests);

  await page.goto('/submission');
  await expect(page.getByRole('cell', { name: /100/ })).toBeVisible();

  await page.getByPlaceholder('Filter by username').fill('bob');
  await expect(page.getByRole('cell', { name: /200/ })).toBeVisible();
  await expect(page.getByRole('cell', { name: /100/ })).toBeHidden();

  const filteredListRequest = requests.find(
    (request) => request.type === 'submissions_list' && request.content.username === 'bob',
  );
  const filteredTotalRequest = requests.find(
    (request) => request.type === 'total_submissions_list_index' && request.content.username === 'bob',
  );
  expect(filteredListRequest?.content.index).toBe(1);
  expect(filteredTotalRequest).toBeDefined();
});

test('keeps the username filter focused while the filtered list is loading', async ({ page }) => {
  await mockWebSocket(page, async (request) => {
    if (request.type === 'total_submissions_list_index') {
      return {
        type: 'total_submissions_list_index',
        content: { request_key: request.content.request_key, total_submissions_list_index: 1 },
      };
    }
    if (request.type === 'submissions_list') {
      if (request.content.username === 'bob') {
        await new Promise((resolve) => setTimeout(resolve, 500));
      }
      return {
        type: 'submissions_list',
        content: {
          request_key: request.content.request_key,
          submissions_list: request.content.username === 'bob'
            ? [{ submission_id: 200, username: 'bob', problem_number: 9, result: 'AC' }]
            : [{ submission_id: 100, username: 'alice', problem_number: 3, result: 'WA' }],
        },
      };
    }
    return authenticatedResponse(request);
  });

  await page.goto('/submission');
  await expect(page.getByRole('cell', { name: /100/ })).toBeVisible();

  const usernameInput = page.getByPlaceholder('Filter by username');
  await usernameInput.fill('bob');
  await expect(usernameInput).toBeFocused();
  await expect(page.getByRole('cell', { name: /200/ })).toBeVisible();
});
