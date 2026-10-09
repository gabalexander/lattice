#pragma once

#define PRESS_ENV "PRESS_CONFIG"
#define MAX_TONS 40

namespace press {

struct Plate {
    int width;
    int height;
};

class Press {
public:
    explicit Press(int tons);
    void stamp(const Plate &plate);
    int tons() const;

private:
    int tons_;
};

}  // namespace press
