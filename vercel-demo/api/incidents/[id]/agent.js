const { buildAgentReport, buildRollbackBrief, readIncident, sendError, sendJson, writeIncident } = require('../../_lib/watchdog');

module.exports = async function handler(req, res) {
  if (req.method !== 'POST') {
    sendJson(res, 405, { error: 'method not allowed' });
    return;
  }
  try {
    const incident = await readIncident(req.query.id);
    if (!incident) {
      sendJson(res, 404, { error: 'incident not found' });
      return;
    }
    const agentReport = buildAgentReport(incident);
    incident.agent_report = agentReport;
    incident.agent_report_updated_at = new Date().toISOString();
    incident.rollback_brief = buildRollbackBrief(incident);
    incident.rollback_brief_updated_at = new Date().toISOString();
    await writeIncident(incident);
    sendJson(res, 200, { agent_report: agentReport, rollback_brief: incident.rollback_brief });
  } catch (error) {
    sendError(res, error);
  }
};
