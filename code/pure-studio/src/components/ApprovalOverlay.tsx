import { Check, ShieldAlert, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
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
    <div className="fixed right-3 bottom-3 z-20 w-[min(440px,calc(100vw-48px))] flex flex-col gap-3">
      {approvals.map((approval) => (
        <Card className="shadow-2xl" key={approval.approvalId}>
          <CardHeader className="pb-3">
            <div className="flex items-start gap-2.5">
              <ShieldAlert size={19} className="mt-0.5 shrink-0" />
              <div className="flex flex-col gap-1.5 min-w-0">
                <div className="flex items-center gap-2 flex-wrap">
                  <CardTitle className="text-sm">{approval.name}</CardTitle>
                  <Badge variant="secondary">{t("subagent.toolLabel")}</Badge>
                </div>
                <p className="text-xs text-muted-foreground break-words">
                  {approval.workingDirectory ?? t("subagent.defaultWorkingDirectory")}
                </p>
                {approval.parentAgentId ? (
                  <p className="text-xs text-muted-foreground">
                    {t("subagent.subagentLabel", { id: approval.parentAgentId })}
                  </p>
                ) : null}
              </div>
            </div>
          </CardHeader>
          <CardContent className="pt-0">
            <ScrollArea className="max-h-40">
              <pre className="text-xs text-muted-foreground bg-muted/50 rounded-md p-3 overflow-x-auto">
                {JSON.stringify(approval.arguments, null, 2)}
              </pre>
            </ScrollArea>
            <Separator className="my-3" />
            <div className="flex justify-end gap-2.5">
              <Button variant="destructive" size="sm" onClick={() => onDeny(approval.approvalId)}>
                <X size={16} />
                {t("actions.deny")}
              </Button>
              <Button size="sm" onClick={() => onApprove(approval.approvalId)}>
                <Check size={16} />
                {t("actions.approve")}
              </Button>
            </div>
          </CardContent>
        </Card>
      ))}
    </div>
  );
}
