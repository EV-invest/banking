"use client";

// The middle column: the book, or the tape, one at a time. Both read the cache the
// socket writes into; the pane itself holds only which of the two is showing.

import { useState } from "react";

import { useT } from "@evinvest/i18n/react";
import { Tabs, TabsList, TabsTrigger, TerminalPane, TerminalPaneBody, TerminalPaneHeader } from "@evinvest/uikit";

import { BookLevels } from "@/views/trade/ui/book-levels";
import { TradesTape } from "@/views/trade/ui/trades-tape";

type BookTab = "book" | "trades";

export function BookPane({ service, onPick }: { service: string; onPick: (price: string) => void }) {
  const t = useT();
  const [tab, setTab] = useState<BookTab>("book");
  return (
    <TerminalPane area="book">
      <TerminalPaneHeader>
        <Tabs value={tab} onValueChange={(v) => setTab(v === "trades" ? "trades" : "book")}>
          <TabsList className="h-7">
            <TabsTrigger value="book" className="text-xs">
              {t("trade.book.title")}
            </TabsTrigger>
            <TabsTrigger value="trades" className="text-xs">
              {t("trade.book.tape")}
            </TabsTrigger>
          </TabsList>
        </Tabs>
      </TerminalPaneHeader>
      <TerminalPaneBody>{tab === "book" ? <BookLevels service={service} onPick={onPick} /> : <TradesTape service={service} />}</TerminalPaneBody>
    </TerminalPane>
  );
}
