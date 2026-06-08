const { listIncidents, sendError, sendJson } = require('./_lib/watchdog');

module.exports = async function handler(_req, res) {
  try {
    const incidents = await listIncidents();
    sendJson(res, 200, {
      status: 'ok',
      storage_backend: 'supabase',
      incident_count: incidents.length,
    });
  } catch (error) {
    sendError(res, error);
  }
};
