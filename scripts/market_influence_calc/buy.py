import random
import math
import sys
import numpy as np

from scipy.optimize import minimize

MAX_INFLUENCE = 30
EXPECTED = {
    100: {
        150: (0.03, 0.25),
        100: (0.25, 0.5),
        50: (0.3, 1.5),
    },
    1000: {
        150: (0.3, 1),
        100: (0.5, 3),
        50: (1.5, 5),
    },
    5000: {
        150: (0.5, 3),
        100: (1.5, 5),
        50: (3, 15),
    },
    15000: {
        150: (1, 5),
        100: (3, 20),
        50: (10, 30),
    },
}

NPARAMS = 6
BOUNDS=[(0.00001, 5.0) for _ in range(NPARAMS)]
X0 = []
for (mn, mx) in BOUNDS: 
    X0.append(mn + random.random() * (mx - mn))
X0 = [0.01201647, 0.62372367, 1.53879355, 0.08393637, 0.02314088, 0.03418173 ]

# AMNT +++ -> INFL +++
# PRIX +++ -> INFL ---
def f(amnt, price, *params):
    assert not any([np.isnan(p) for p in params])
    a, b, c, d, e, param_f = params # Renommé de 'f' à 'param_f' pour éviter l'écrasement de la fonction
    
    norm_amnt = amnt / 1000.0
    norm_price = price / 100.0
    
    safe_amnt = max(norm_amnt, 1e-6)
    safe_price = max(norm_price, 1e-6)
    
    # Modèle d'achat : amount au numérateur, prix au dénominateur
    base = a * (safe_amnt ** b) / (safe_price ** c)
    multiplier = 1.0 + (d * (safe_amnt ** e) / (safe_price ** param_f))
    
    return base * multiplier

def fit_parameters(expected_data):
    points = []
    for amnt, prices in expected_data.items():
        for price, (startr, endr) in prices.items():
            min_v = startr / 100.0
            max_v = endr / 100.0
            # Si startr est 0, on prend une petite valeur pour le milieu logarithmique
            effective_min = max(min_v, 0.0001)
            mid = (effective_min + max_v) / 2.0
            points.append((amnt, price, mid, min_v, max_v))

    def loss(params):
        total_err = 0.0
        for amnt, price, mid, min_v, max_v in points:
            pred = f(amnt, price, *params)
            # Écart quadratique par rapport au milieu de la plage
            if pred < 0:
                total_err += ((1+abs(pred)) * 10) ** 2
            else:
                total_err += (math.log(pred) - math.log(mid)) ** 2
            
            # Pénalité en cas d'échec des assertions
            if pred < min_v:
                total_err += ((min_v - pred) * 3) ** 2
            elif pred > max_v:
                total_err += ((pred - max_v) * 3) ** 2
        return total_err
    res = minimize(
        loss, 
        x0=X0,
        bounds=BOUNDS,
    )
    return res.x

# 1. Calcul automatique des paramètres
params = fit_parameters(EXPECTED)
print("Paramètres trouvés:", end="")
for (n, p) in enumerate(params):
    char = chr(ord("a") + n)
    print(f"{char}={p} ", end="")
print("")

print(6, 6 * (1 + f(1000, 90, *params)))
print(6, 6 * (1 + f(2000, 90, *params)))

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

regressions = []
n = 0
while True:
    if n < len(regressions):
        seed = regressions[n]
        random.seed(seed)
    else:
        random.seed(None)
        seed = int(random.random() * 100000000000000000)

    max_exp = max(EXPECTED.keys())
    amnt = (0.5 + (random.random()/2)) * random.randrange(1, int(max_exp * 1.3))
    currp = (random.random() + 0.5) * 100
    try:
        assert infl < 1, "Influence < 1"
        assert infl >= 0, "Influence >= 0"
        if infl < MAX_INFLUENCE / 100:
            assert f(amnt, price, *params) < f(amnt * 1.1, price, *params), "Price increase"
            assert f(amnt, price * 0.9, *params) > f(amnt, price * 1.1, *params), "Higher incease when lower price" 
            assert f(amnt, price * 1.1, *params) < f(amnt * 1.1, price * 0.9, *params), "Mixed"
    except AssertionError as err:
        print(f"{n}| Seed {seed} failed: {err}")
        print(f"\tPrice  {currp}");
        print(f"\tAmount {amnt}")
        sys.exit(0)
    n += 1
