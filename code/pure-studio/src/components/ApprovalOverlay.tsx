import { Check, ShieldAlert, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { ToolApprovalRequest } from "../types";

type ApprovalOverlayProps = {
  approvals: ToolApprovalRequest[];
  onApprove: (id: string) => void;
  onDeny: (id: string) => void;
};

export function ApprovalOverlay({ approvals, onApprove, onDeny }: ApprovalOverlayProps) {
  const { t } = useTranslation();

  if (approvals.length === 0) {
    return null;
  }

  return (
    <div className="approval-stack">
      {approvals.map((approval) => (
        <section className="approval-card" key={approval.approvalId}>
          <div className="approval-heading">
            <ShieldAlert size={19} />
            <div>
              <strong>{approval.name}</strong>
              <span>{approval.workingDirectory ?? t("subagent.defaultWorkingDirectory")}</span>
              {approval.parentSubagentId ? (
                <span>{t("subagent.subagentLabel", { id: approval.parentSubagentId })}</span>
              ) : null}
            </div>
          </div>
          <pre>{JSON.stringify(approval.arguments, null, 2)}</pre>
          <div className="approval-actions">
            <button onClick={() => onDeny(approval.approvalId)}>
              <X size={16} />
              {t("actions.deny")}
            </button>
            <button className="primary" onClick={() => onApprove(approval.approvalId)}>
              <Check size={16} />
              {t("actions.approve")}
            </button>
          </div>
        </section>
      ))}
    </div>
  );
}
