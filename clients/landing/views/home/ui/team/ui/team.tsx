import { getVariant } from "@/features/ab-variant/get-variant";
import { ExperimentTracker, match } from "@/features/ab-variant";
import { TeamA } from "./a";
import { TeamB } from "./b";

/**
 * Server wrapper — owns the Team's A/B decision.
 *
 * `HomeView` renders `<Team />` and is unaware that it's tested. The chosen
 * variant is created server-side so only its chunk reaches the client.
 * {@link ExperimentTracker} fires the exposure event and provides variant
 * context to the {@link TeamPlaceholders} client island inside each variant.
 */
export async function Team() {
  const variant = await getVariant("team");
  return (
    <ExperimentTracker experiment="team" variant={variant}>
      {match(variant, { a: <TeamA />, b: <TeamB /> })}
    </ExperimentTracker>
  );
}
