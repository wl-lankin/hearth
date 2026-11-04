/**
 * The house on Home. A dark picture is the base; each window shows the lit picture through its own
 * rectangle while what it stands for runs. Rectangles are percentages of the picture.
 */

export type Win = { id: string; x: number; y: number; w: number; h: number };

/** The windows, in the order they light up: the terminal for Hearth's web server, then one per service. */
export const windows: Win[] = [
  { id: 'terminal', x: 50.5, y: 64.8, w: 10, h: 16.8 },
  { id: 'dormer', x: 37.8, y: 46.8, w: 4, h: 7.6 },
  { id: 'left', x: 33, y: 62.5, w: 5, h: 12.6 },
  // The door panes and the lantern beside them.
  { id: 'door', x: 39.2, y: 70.6, w: 7, h: 7 },
];

/** The server rack's LEDs, around the terminal window: they light when services outnumber windows. */
export const rack: Win[] = [
  { id: 'rack-top', x: 50, y: 44.5, w: 20.5, h: 19.5 },
  { id: 'rack-side', x: 61, y: 59, w: 9.5, h: 34 },
  { id: 'rack-bottom', x: 50, y: 81.6, w: 11.2, h: 12 },
];

/** Which windows are lit, and whether the rack is, for the web server and a number of running services. */
export function litWindows(webRunning: boolean, runningServices: number): { windows: string[]; rack: boolean } {
  const services = windows.slice(1);
  const count = Math.max(0, Math.floor(runningServices));
  return {
    windows: [...(webRunning ? ['terminal'] : []), ...services.slice(0, count).map((w) => w.id)],
    rack: count > services.length,
  };
}

export function greeting(hour: number): string {
  if (hour >= 5 && hour < 12) return 'Good morning';
  if (hour >= 12 && hour < 18) return 'Good afternoon';
  if (hour >= 18 && hour < 23) return 'Good evening';
  return 'Good night';
}
