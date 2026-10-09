"""The oven."""

MAX_HEAT = 1300
HEAT_ENV = "KILN_HEAT"


class Oven:
    """An oven that fires pots."""

    door: str = "closed"

    def fire(self, pot):
        """Fires `pot`."""
        def inner():
            pass
        return inner


def cool(oven):
    return oven
