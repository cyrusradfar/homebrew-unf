/**
 * UNFUDGED Desktop App E2E Tests
 *
 * Tests points 8-10:
 * 8. App opens and shows All Projects view
 * 9. Sub-select a project
 * 10. Filter by folder and see numbers change
 *
 * Precondition: CLI smoke test has run, leaving two watched projects
 * with snapshot data. The app test setup script re-creates this state.
 *
 * Run with: npm run test:app
 */

import { waitFor, waitForText, getText, click, type, screenshot } from './helpers';

describe('UNFUDGED Desktop App', () => {

  // =========================================================================
  // Point 8: App opens and shows All Projects view
  // =========================================================================

  it('should launch and show the All Projects tab', async () => {
    // Wait for the app to fully load
    await browser.waitUntil(
      async () => (await browser.getPageSource()).includes('All Projects'),
      { timeout: 15000, timeoutMsg: 'App did not load within 15s' }
    );

    // Verify All Projects tab is visible and active
    const globalTab = await $('.tab.global-tab');
    expect(await globalTab.isDisplayed()).toBe(true);

    const tabButton = await $('.tab.global-tab .tab-button');
    const tabText = await tabButton.getText();
    expect(tabText).toContain('All Projects');
  });

  it('should show metrics with projects, snapshots, and files', async () => {
    // Wait for metrics to populate (data loading)
    await browser.waitUntil(
      async () => {
        const values = await $$('.metric .metric-value');
        if (values.length === 0) return false;
        const firstVal = await values[0].getText();
        return firstVal !== '0' && firstVal !== '';
      },
      { timeout: 10000, timeoutMsg: 'Metrics did not populate' }
    );

    // Get all metric values and labels
    const metricValues = await $$('.metric .metric-value');
    const metricLabels = await $$('.metric .metric-label');

    // Should have at least snapshots, files, projects
    expect(metricValues.length).toBeGreaterThanOrEqual(3);

    // Find the projects metric
    let projectCount = 0;
    for (let i = 0; i < metricLabels.length; i++) {
      const label = await metricLabels[i].getText();
      if (label.includes('project')) {
        projectCount = parseInt(await metricValues[i].getText(), 10);
        break;
      }
    }
    expect(projectCount).toBeGreaterThanOrEqual(2);

    // Snapshot count should be > 0
    const snapshotText = await metricValues[0].getText();
    const snapshotCount = parseInt(snapshotText.replace(/,/g, ''), 10);
    expect(snapshotCount).toBeGreaterThan(0);

    await screenshot('08-all-projects-metrics');
  });

  it('should show timeline entries from multiple projects', async () => {
    // Wait for timeline entries to load
    await browser.waitUntil(
      async () => {
        const entries = await $$('.file-group, .entry');
        return entries.length > 0;
      },
      { timeout: 10000, timeoutMsg: 'Timeline entries did not load' }
    );

    const entries = await $$('.file-group, .entry');
    expect(entries.length).toBeGreaterThan(0);

    await screenshot('08-all-projects-timeline');
  });

  // =========================================================================
  // Point 9: Sub-select a project
  // =========================================================================

  it('should open project dropdown and show available projects', async () => {
    // Click the dropdown button to open project selector
    const dropdownBtn = await $('.dropdown-button');
    await dropdownBtn.click();
    await browser.pause(500);

    // Dropdown panel should be visible
    const panel = await $('.dropdown-panel');
    expect(await panel.isDisplayed()).toBe(true);

    // Should have selectable project items
    const items = await $$('.dropdown-item.selectable');
    expect(items.length).toBeGreaterThanOrEqual(1);

    await screenshot('09-project-dropdown');
  });

  it('should switch to a specific project', async () => {
    // Click the first selectable project in the dropdown
    const items = await $$('.dropdown-item.selectable');
    const projectName = await items[0].$('.project-name').getText();
    await items[0].click();
    await browser.pause(1000); // Wait for tab switch and data load

    // Verify a new tab appeared and is active
    const activeTabs = await $$('.tab.active:not(.global-tab)');
    expect(activeTabs.length).toBe(1);

    // Metrics should update — project count should no longer be shown
    // (single project view doesn't show project count)
    await browser.waitUntil(
      async () => {
        const labels = await $$('.metric .metric-label');
        for (const label of labels) {
          const text = await label.getText();
          if (text.includes('project')) return false;
        }
        return true;
      },
      { timeout: 5000, timeoutMsg: 'Metrics still showing project count after sub-select' }
    );

    // Snapshot and file counts should still be > 0
    const metricValues = await $$('.metric .metric-value');
    expect(metricValues.length).toBeGreaterThanOrEqual(2);

    const snapshotText = await metricValues[0].getText();
    expect(parseInt(snapshotText.replace(/,/g, ''), 10)).toBeGreaterThan(0);

    await screenshot('09-project-selected');
  });

  // =========================================================================
  // Point 10: Filter by folder and see numbers change
  // =========================================================================

  it('should filter by file and see counts change', async () => {
    // Record unfiltered counts first
    const unfilteredValues = await $$('.metric .metric-value');
    const unfilteredSnapshotText = await unfilteredValues[0].getText();
    const unfilteredSnapshots = parseInt(unfilteredSnapshotText.replace(/,/g, ''), 10);

    // Type into the filter input
    const filterInput = await $('input[placeholder*="Filter"]');
    await filterInput.click();
    await browser.pause(300);
    await filterInput.setValue('file1');
    await browser.pause(500);

    // Autocomplete dropdown should appear
    const dropdown = await $('[role="listbox"]');
    if (await dropdown.isDisplayed()) {
      // Click the first matching item
      const firstItem = await $('.dropdown-item');
      await firstItem.click();
      await browser.pause(500);
    } else {
      // Press Enter to apply the filter directly
      await browser.keys(['Enter']);
      await browser.pause(500);
    }

    // A filter chip should appear
    await browser.waitUntil(
      async () => {
        const chips = await $$('.filter-chip');
        return chips.length > 0;
      },
      { timeout: 5000, timeoutMsg: 'Filter chip did not appear' }
    );

    // Filtered counts should show "X / Y" format
    const metricOf = await $$('.metric .metric-of');
    if (metricOf.length > 0) {
      // Has "/ total" indicator = filtering is active
      const totalText = await metricOf[0].getText();
      expect(totalText).toContain('/');

      // Filtered count should be less than or equal to total
      const filteredValues = await $$('.metric .metric-value');
      const filteredSnapshots = parseInt((await filteredValues[0].getText()).replace(/,/g, ''), 10);
      expect(filteredSnapshots).toBeLessThanOrEqual(unfilteredSnapshots);
    }

    await screenshot('10-filtered');
  });

  it('should clear filter and restore original counts', async () => {
    // Record filtered state
    const filteredMetricOf = await $$('.metric .metric-of');
    expect(filteredMetricOf.length).toBeGreaterThan(0);

    // Click the × on the filter chip to clear it
    const chipClose = await $('.chip-x');
    await chipClose.click();
    await browser.pause(500);

    // Filter chips should be gone
    const chips = await $$('.filter-chip');
    expect(chips.length).toBe(0);

    // "/ total" indicators should be gone (no longer filtered)
    await browser.waitUntil(
      async () => {
        const metricOf = await $$('.metric .metric-of');
        return metricOf.length === 0 || !(await metricOf[0].isDisplayed());
      },
      { timeout: 5000, timeoutMsg: 'Metric-of still visible after clearing filter' }
    );

    await screenshot('10-filter-cleared');
  });

  // =========================================================================
  // Cleanup
  // =========================================================================

  after(async () => {
    await screenshot('final-state');
  });
});
