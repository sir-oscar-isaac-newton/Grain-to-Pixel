import { useEffect, useState } from 'react';
import { updateCheck } from '../../shared/ipc';
import { UpdateOffer, type UpdateOfferInfo } from './UpdateOffer';

/**
 * Asks for an update once when the app starts (the backend answers "not due" within a day of the
 * last check and "disabled" when the check is switched off) and offers one that is found. A check
 * that fails says nothing: there is nothing to do about a missing connection at launch.
 */
export function UpdateNotice() {
  const [offer, setOffer] = useState<UpdateOfferInfo | null>(null);

  useEffect(() => {
    let alive = true;
    updateCheck(false).then(
      (outcome) => {
        if (alive && outcome.status === 'available') setOffer(outcome);
      },
      () => {
        // Offline, or no backend (tests, plain browser): the next start asks again.
      },
    );
    return () => {
      alive = false;
    };
  }, []);

  return offer && <UpdateOffer offer={offer} onLater={() => setOffer(null)} />;
}
