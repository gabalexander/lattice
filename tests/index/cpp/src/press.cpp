#include "press.h"

namespace press {

Press::Press(int tons) : tons_(tons) {}

void Press::stamp(const Plate &plate) {
    (void)plate;
}

int Press::tons() const {
    return tons_;
}

}  // namespace press
