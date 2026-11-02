/** A short uptime with the two largest units: "42s", "2m 5s", "2h 5m", "1d 1h". */
export function formatUptime(totalSecs: number): string {
  const days = Math.floor(totalSecs / 86400);
  const hours = Math.floor((totalSecs % 86400) / 3600);
  const minutes = Math.floor((totalSecs % 3600) / 60);
  const secs = totalSecs % 60;
  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${minutes}m`;
  if (minutes > 0) return `${minutes}m ${secs}s`;
  return `${secs}s`;
}

/** When a mail arrived: the time today, the day this year, else the day and year. */
export function formatReceived(unixSecs: number, now: Date = new Date()): string {
  const at = new Date(unixSecs * 1000);
  const pad = (n: number) => String(n).padStart(2, '0');
  if (at.toDateString() === now.toDateString()) return `${pad(at.getHours())}:${pad(at.getMinutes())}`;
  const day = `${at.getDate()} ${at.toLocaleString('en-GB', { month: 'short' })}`;
  return at.getFullYear() === now.getFullYear() ? day : `${day} ${at.getFullYear()}`;
}

/** A file size: "900 B", "40 KB", "3.0 MB". */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}
