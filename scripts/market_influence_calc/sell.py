import random
import math
import sys
import numpy as np

from scipy.optimize import minimize

MAX_INFLUENCE = 30
EXPECTED = {
    100: {
        50: (0, 0.25),
        100: (0.25, 1.25),
        150: (1, 3),
    },
    1000: {
        50: (0.5, 1),
        100: (1, 3),
        150: (3, 10),
    },
    5000: {
        50: (1, 3),
        100: (3, 10),
        150: (10, 20),
    },
    15000: {
        50: (1, 5),
        100: (3, 20),
        150: (10, 30),
    },
}

NPARAMS = 6
BOUNDS=[(0.0001, 2.0) for _ in range(NPARAMS)]
X0 = []
for (mn, mx) in BOUNDS: 
    X0.append((mx + mn) / 2.0)
X0 = [0.02578237202951275, 0.5554258819004403, 1.9919988458913422, 0.0018077576687068689, 0.010908391555243003, 0.002494236546541542]

# AMNT +++ -> INFL +++
# PRIX +++ -> INFL +++
def f(amnt, price, *params):
    assert not any([np.isnan(p) for p in params])
    a, b, c, d, e, f = params
    
    norm_amnt = amnt / 1000.0
    norm_price = price / 100.0
    
    safe_amnt = max(norm_amnt, 1e-6)
    safe_price = max(norm_price, 1e-6)
    
    # Modèle à loi de puissance combinée garantissant une croissance monotone
    base = a * (safe_amnt ** b) * (safe_price ** c)
    multiplier = 1.0 + d * (safe_amnt ** e) * (safe_price ** f)
    
    return base * multiplier

def fit_parameters(expected_data):
    points = []
    for amnt, prices in expected_data.items():
        for price, (startr, endr) in prices.items():
            min_v = startr / 100.0
            max_v = endr / 100.0
            assert (MAX_INFLUENCE / 100) >= max_v
            assert (MAX_INFLUENCE / 100) >= min_v
            effective_min = max(min_v, 0.0001)
            mid = (effective_min + max_v) / 2.0
            points.append((amnt, price, mid, min_v, max_v))

    def loss(params):
        total_err = np.float64(0)
        for amnt, price, mid, min_v, max_v in points:
            pred = f(amnt, price, *params)
            if pred > (MAX_INFLUENCE / 100):
                over = pred - (MAX_INFLUENCE / 100)
                total_err += np.float64((pred + math.log(1.0 + over)) * 10) ** 2
            else:
                try:
                    total_err += np.float64(math.log(pred) - math.log(mid)) ** 2
                except ValueError:
                    total_err += np.float64(1e10 * abs(pred))

            if pred < min_v:
                total_err += np.float64(((min_v - pred) * 10) ** 2)
            elif pred > max_v:
                total_err += np.float64(((pred - max_v) * 10) ** 2)
        return total_err

    res = minimize(loss, x0=X0, bounds=BOUNDS)
    print(res.status, res.message, res.nit)
    return res.x

params = fit_parameters(EXPECTED)
print("Paramètres trouvés:", end="")
for (n, p) in enumerate(params):
    char = chr(ord("a") + n)
    print(f"{char}={p} ", end="")
print("")

try:
    for amnt, data in EXPECTED.items():
        for price, (startr, endr) in data.items():
            infl = f(amnt, price, *params)
            infls = round(infl * 100, 2)
            assert infl >= startr / 100.0, f"Amnt {amnt} price {price}%, influence {infls}% < min {startr}%"
            assert infl <= endr / 100.0, f"Amnt {amnt} price {price}%, influence {infls}% > max {endr}%"
            print(f"* {amnt}\t{price}:\t{infls}%\tOK")
except AssertionError as err:
    print(f"Basic assertion failed: {err}")
    sys.exit(0)

print(1 - f(1000, 90, *params))
print(6, 6 * (1 - f(1000, 90, *params)))
print(6, 6 * (1 - f(2000, 90, *params)))
print("")

regressions = [
    64829259514998640,
]
n = 0
while True:
    if n < len(regressions):
        seed = regressions[n]
        random.seed(seed)
    else:
        random.seed(None)
        seed = int(random.random() * 100000000000000000)

    max_exp = max(EXPECTED.keys())
    amnt = (0.5 + (random.random()/2)) * random.randrange(20, int(max_exp * 1.1))
    currp = (random.random() + 0.5) * 100
    try:
        infl = f(amnt, price, *params)
        assert infl < 1, "Influence < 1"
        assert infl >= 0, "Influence >= 0"
        if infl < MAX_INFLUENCE / 100:
            assert f(amnt, price, *params) < f(amnt * 1.1, price, *params), "Influence increase when amnt increase"
            assert f(amnt, price * 0.9, *params) < f(amnt, price * 1.1, *params), "Influence increase when price increase"
    except AssertionError as err:
        print(f"{n}| Seed {seed} failed: {err}")
        print(f"\tPrice      {currp}")
        print(f"\tAmount     {amnt}")
        print(f"\tInfluence  {infl}")
        print(f"\tPrice mult", 1 - infl)
        print(f"\tParams     a={a} b={b} c={C}")
        sys.exit(0)
    n += 1
