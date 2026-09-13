"use client";

// Admin console — PAYMENTS: money between two named ends of the platform.
//
// Three tiers, told apart by the destination: inside the fund between two claims, into
// one of its products, or out to an address. Who has to agree is told by the SOURCE:
// fund-owned money asks the owners' consilium, an investor's own claim asks that investor
// by email. The form previews both; the plane decides both; the list shows the plane's.
//
// This is where the revenue screen's "propose a payout" went. That form could only pay
// the fund's earnings out to an address; an order here names any two ends, and the
// revenue screen is statistics now, linking back here.

import { useT } from "@evinvest/i18n/react";
import { Card, CardContent } from "@evinvest/uikit";

import { StaggerItem } from "@/shared/ui/motion";
import { OpenPaymentForm } from "@/views/admin/payments/ui/open-payment-form";
import { PaymentList } from "@/views/admin/payments/ui/payment-list";
import { AdminHeader, AdminScreen } from "@/views/admin/ui/shell";

export function PaymentsView() {
  const t = useT();
  return (
    <AdminScreen className="space-y-8">
      <AdminHeader eyebrow={t("admin.eyebrow.administer")} title={t("nav.payments")} subtitle={t("admin.payments.subtitle")} />

      <StaggerItem as="section" className="space-y-3">
        <p className="text-xs font-semibold uppercase tracking-widest text-muted-foreground">{t("admin.payments.open")}</p>
        <Card>
          <CardContent className="py-5">
            <OpenPaymentForm />
          </CardContent>
        </Card>
      </StaggerItem>

      <StaggerItem as="section">
        <PaymentList />
      </StaggerItem>
    </AdminScreen>
  );
}
