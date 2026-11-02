import os
import sys
import math
import time
import json
import string
import urllib.request

class SimeisError(Exception):
    pass

def get_dist(a, b):
    return math.sqrt(((a[0]-b[0]) ** 2) + ((a[1]-b[1]) ** 2) + ((a[2]-b[2]) ** 2))

# Check if types are present in the list
def check_has(alld, key, *req):
    alltypes = [c[key] for c in alld.values()]
    return all([k in alltypes for k in req])

class SimeisSDK:
    def __init__(self, username, ip, port, debug=False):
        self.url = f"http://{ip}:{port}"
        self.debug = debug
        assert self.api("/ping")["ping"] == "pong"
        self.setup_player(username)
        self.game_config = self.get_game_config()

    def config(self, key):
        val = self.game_config
        for k in key.split("."):
            val = val[k]
        return val

    def api(self, path, method="GET", timeout=5, body=None, **qry):
        if self.debug:
            print(method, path)

        tail = ""
        if len(qry) > 0:
            tail += "?"
            tail += "&".join([
                "{}={}".format(k, urllib.parse.quote(v)) for k, v in qry.items()
            ])

        qry = f"{self.url}{path}{tail}"

        hdr = {}
        if hasattr(self, "player"):
            hdr["Simeis-Key"] = self.player["key"]
        req = urllib.request.Request(qry, headers=hdr, method=method)
        if body is not None:
            req.data = json.dumps(body).encode()
            req.add_header("Content-Type", "application/json")

        reply = urllib.request.urlopen(req, timeout=timeout)

        data = json.loads(reply.read().decode())
        err = data.pop("error")
        if err != "ok":
            raise SimeisError(err)

        return data

    def get(self, *args, **kwargs):
        return self.api(*args, method="GET", **kwargs)

    def post(self, *args, body=None, **kwargs):
        return self.api(*args, method="POST", body=body, **kwargs)
        
    # If we have a file containing the player ID and key, use it
    # If not, create a new player
    # If the player has lost, print an error message
    def setup_player(self, username, force_register=False):
        # Sanitize the username, remove any symbols
        username = "".join([c for c in username if c in string.ascii_letters + string.digits]).lower()

        # If we don't have any existing account
        if force_register or not os.path.isfile(f"./{username}.json"):
            player = self.post(f"/player/new/{username}")
            with open(f"./{username}.json", "w") as f:
                json.dump(player, f, indent=2)       
            self.player = player

        # If an account already exists
        else:
            with open(f"./{username}.json", "r") as f:
                self.player = json.load(f)

        # Try to get the profile
        try:
            id = self.player["playerId"]
            player = self.get(f"/player/{id}")

        # If we fail, that must be that the player doesn't exist on the server
        except SimeisError:
            # And so we retry but forcing to register a new account
            return self.setup_player(username, force_register=True)

        # If the player already failed, we must reset the server
        # Or recreate an account with a new nickname
        if player["money"] <= 0.0:
            print("!!! Player already lost, please restart the server to reset the game")
            sys.exit(0)

    def ping(self):
        return self.get("/ping")

    def get_player_status(self):
        id = str(self.player["playerId"])
        return self.get(f"/player/{id}")

    def get_ship_status(self, ship_id):
        return self.get(f"/ship/{ship_id}")

    def get_station_status(self, sta):
        return self.get(f"/station/{sta}")

    def shop_list_modules(self, sta):
        return self.get(f"/station/{sta}/shop/modules")

    def shop_list_ship(self, sta):
        all = self.get(f"/station/{sta}/shipyard/list")["ships"]
        return sorted(all, key = lambda ship: ship["price"])

    def buy_ship(self, sta, shipid):
        return self.post(f"/station/{sta}/shipyard/buy/{shipid}")

    def buy_module_on_ship(self, sta, shipid, modtype):
        return self.post(f"/station/{sta}/shop/modules/{shipid}/buy/{modtype}")

    def hire_crew(self, sta, crewtype):
        return self.post(f"/station/{sta}/crew/hire/{crewtype.lower()}")

    def fire_crew(self, sta, crew_id):
        return self.post(f"/station/{sta}/crew/fire/{crew_id}")

    def assign_crew_to_ship(self, sta, shipid, operator_id, role):
        return self.post(f"/station/{sta}/crew/assign/{operator_id}/ship/{shipid}/{role}")

    def station_has_trader(self, sta):
        station = self.get(f"/station/{sta}")
        return check_has(station["crew"], "member_type", "Trader")

    def assign_trader_to_station(self, sta, trader_id):
        return self.post(f"/station/{sta}/crew/assign/{trader_id}/trading")

    def compute_travel_cost(self, origin, destination, ship_stats, cargo_usage_perc):
        return self.post(f"/calc/navigation", body={
            "origin": origin,
            "dest": destination,
            "ship_stats":  ship_stats,
            "cargo_usage_perc": cargo_usage_perc,
        })

    def travel(self, ship_id, position, wait_end=True):
        x, y, z = position
        costs = self.post(f"/ship/{ship_id}/navigate", body={"x": x, "y": y, "z": z})
        if wait_end:
            time.sleep(costs["duration"])
            self.wait_until_ship_idle(ship_id)
        return costs

    def stop_travel(self, ship_id):
        return self.post(f"/ship/{ship_id}/navigation/stop")

    def wait_until_ship_idle(self, ship_id, ts=0.2):
        ship = self.get(f"/ship/{ship_id}")
        while ship["state"] != "Idle":
            time.sleep(ts)
            ship = self.get(f"/ship/{ship_id}")

    def buy_hull_for_repair(self, sta, ship_id):
        ship = self.get(f"/ship/{ship_id}")
        req = int(ship["hull_decay"])
        # Pas besoin
        if req == 0:
            return None

        cargo = self.get(f"/station/{sta}")["cargo"]
        if "hull" not in cargo["resources"]:
            cargo["resources"]["hull"] = 0

        if cargo["resources"]["hull"] < req:
            need = req - cargo["resources"]["hull"]
            return self.post(f"/market/{sta}/buy/hull/{need}")
        return None

    def repair_ship(self, sta, ship_id):
        ship = self.get(f"/ship/{ship_id}")
        req = int(ship["hull_decay"])

        # Pas besoin
        if req == 0:
            return None

        cargo = self.get(f"/station/{sta}")["cargo"]
        if "hull" not in cargo["resources"]:
            cargo["resources"]["hull"] = 0

        if cargo["resources"]["hull"] > 0:
            return self.post(f"/station/{sta}/repair/{ship_id}")

        return None

    def buy_fuel_for_refuel(self, sta, ship_id):
        ship = self.get(f"/ship/{ship_id}")
        req = int(ship["fuel_tank_capacity"] - ship["fuel_tank"])

        # Pas besoin
        if req == 0:
            return

        cargo = self.get(f"/station/{sta}")["cargo"]
        if "fuel" not in cargo["resources"]:
            cargo["resources"]["fuel"] = 0

        if cargo["resources"]["fuel"] < req:
            need = req - cargo["resources"]["fuel"]
            return self.post(f"/market/{sta}/buy/fuel/{need}")
        return None

    def refuel_ship(self, sta, ship_id):
        ship = self.get(f"/ship/{ship_id}")
        req = int(ship["fuel_tank_capacity"] - ship["fuel_tank"])

        # Pas besoin
        if req == 0:
            return

        cargo = self.get(f"/station/{sta}")["cargo"]
        if "fuel" not in cargo["resources"]:
            cargo["resources"]["fuel"] = 0        

        if cargo["resources"]["fuel"] > 0:
            return self.post(f"/station/{sta}/refuel/{ship_id}")
        return None

    def scan_planets(self, sta):
        station = self.get(f"/station/{sta}")
        planets = self.post(f"/station/{sta}/scan")["planets"]
        return  sorted(planets,
            key=lambda pla: get_dist(station["position"], pla["position"])
        )

    def start_extraction(self, ship_id):
        return self.post(f"/ship/{ship_id}/extraction/start")

    def stop_extraction(self, ship_id):
        return self.post(f"/ship/{ship_id}/extraction/stop")

    def unload(self, sta, ship_id, res, amnt):
        return self.post(f"/ship/{ship_id}/unload/{sta}/{res}/{amnt}")

    def unload_all(self, sta, ship_id):
        return self.post(f"/ship/{ship_id}/unload/{sta}/all")

    def return_station_and_unload_all(self, sta, ship_id):
        ship = self.get(f"/ship/{ship_id}")
        station = self.get(f"/station/{sta}")
        result = {}
        if ship["position"] != station["position"]:
            result["travel"] = self.travel(ship["id"], station["position"])
        result["unload"] = self.unload_all(sta, ship_id)
        return result

    def get_station_resources(self, sta):
        return self.get(f"/station/{sta}")["cargo"]["resources"]

    def get_market_prices(self):
        return self.get("/market/prices")

    def sell_resource(self, sta, res, amnt):
        return self.post(f"/market/{sta}/sell/{res}/{amnt}")

    def buy_resource(self, sta, res, amnt):
        return self.post(f"/market/{sta}/buy/{res}/{amnt}")

    def get_syslogs(self):
        return self.get("/syslogs")["events"]

    def get_resources_info(self):
        return self.get("/resources")

    def get_ship_wages_cost(self, ship_id):
        return self.get(f"/ship/{ship_id}/wages")["wages"]

    def list_ship_crew_upgrades(self, sta, ship_id):
        return self.get(f"/station/{sta}/crew/upgrade/ship/{ship_id}")

    def list_ship_module_upgrades(self, sta, ship_id):
        return self.get(f"/station/{sta}/shop/modules/{ship_id}/upgrade")

    def list_ship_upgrades(self, sta, ship_id):
        return self.get(f"/station/{sta}/shipyard/upgrade/{ship_id}")

    def list_station_upgrades(self, sta):
        return self.get(f"/station/{sta}/upgrades")

    def upgrade_ship_crew(self, sta, ship_id, crew_id):
        return self.post(f"/station/{sta}/crew/upgrade/ship/{ship_id}/{crew_id}")

    def upgrade_ship_module(self, sta, ship_id, mod_id):
        return self.post(f"/station/{sta}/shop/modules/{ship_id}/upgrade/{mod_id}")

    def upgrade_station_cargo(self, sta, ncargo):
        return self.post(f"/station/{sta}/shop/cargo/buy/{ncargo}")

    def upgrade_station_crew(self, sta, crew_id):
        return self.post(f"/station/{sta}/crew/upgrade/{crew_id}")

    def buy_ship_upgrade(self, sta, ship_id, upgtype):
        return self.post(f"/station/{sta}/shipyard/upgrade/{ship_id}/{upgtype}")

    def buy_ship_expansion(self, sta, ship_id, exptype, n):
        return self.post(f"/station/{sta}/shipyard/upgrade/{ship_id}/expand/{exptype}/{n}")

    def compute_mining_rate(self, modtype, modrank, oprank, planet, power_attr={}):
        return  self.post("/calc/mining_rate", body={
            "module_type": modtype,
            "module_rank": modrank,
            "operator_rank": oprank,
            "planet_coord": planet,
            "power_attribution": power_attr,
        })

    def compute_ship_mining_rate(self, ship_id, planet, power_attr={}):
        return  self.post(f"/calc/mining_rate/{ship_id}", body={
            "module_type": modtype,
            "module_rank": modrank,
            "operator_rank": oprank,
            "planet_coord": planet,
            "power_attribution": power_attr,
        })

    def compute_upgrade_ship(self, start_reactor, start_shield, start_pilot, start_cargo_capacity, end_reactor=None, end_shield=None, end_pilot=None, end_cargo_capacity=None):
        if end_reactor is None:
            end_reactor = start_reactor
        if end_shield is None:
            end_shield = start_shield
        if end_pilot is None:
            end_pilot = start_pilot
        if end_cargo_capacity is None:
            end_cargo_capacity = start_cargo_capacity
        return self.post("/calc/upgrade/ship", body={
            "start_reactor_rank": start_reactor,
            "end_reactor_rank": end_reactor,
            "start_shield_rank": start_shield,
            "end_shield_rank": end_shield,
            "start_pilot_rank": start_pilot,
            "end_pilot_rank": end_pilot,
            "start_cargo_capacity": start_cargo_capacity,
            "end_cargo_capacity": end_cargo_capacity,
        })

    def compute_upgrade_module(self, modtype, mod_start_rank, crew_start_rank, mod_end_rank=None, crew_end_rank=None):
        if mod_end_rank is None:
            mod_end_rank = mod_start_rank
        if crew_end_rank is None:
            crew_end_rank = crew_start_rank
        return self.post("/calc/upgrade/module", body={
            "module_type": modtype,
            "module_start_rank": mod_start_rank,
            "module_end_rank": mod_end_rank,
            "crew_start_rank": crew_start_rank,
            "crew_end_rank": crew_end_rank,
        })

    def compute_upgrade_trader(self, start, end):
        return self.post("/calc/upgrade/trader", body={
            "trader_start_rank": start,
            "trader_end_rank": end,
        })

    def game_version(self):
        return self.get("/version")

    def get_game_stats(self):
        return self.get("/gamestats")

    def get_fee_rate(self, sta):
        return self.get(f"/market/{sta}/fee_rate")

    def list_industry_to_buy(self, sta):
        return self.get(f"/station/{sta}/industry/buy")

    def buy_industry(self, sta, name):
        return self.post(f"/station/{sta}/industry/buy/{name}")

    def get_industry_production(self, sta, iid):
        return self.get(f"/station/{sta}/industry/production/{iid}")

    def assign_crew_to_industry(self, sta, crew_id, iid):
        return self.post(f"/station/{sta}/crew/assign/{crew_id}/industry/{iid}")

    def start_industry(self, sta, iid):
        return self.post(f"/station/{sta}/industry/start/{iid}")

    def stop_industry(self, sta, iid):
        return self.post(f"/station/{sta}/industry/stop/{iid}")

    def upgrade_industry(self, sta, iid):
        return self.post(f"/station/{sta}/industry/upgrade/{iid}")

    def get_extraction_power_attribution(self, ship_id):
        return self.get(f"/ship/{ship_id}/extraction/power_attribution")

    def set_extraction_power_attribution(self, ship_id, res, pow):
        return self.post(f"/ship/{ship_id}/extraction/power_attribution/{res}/{pow}")

    def get_station_wages(self, sta):
        return self.get(f"/station/{sta}/wages")

    def get_all_wages(self):
        return self.get("/player/wages")

    def unassign_crew(self, sta, id):
        return self.post(f"/station/{sta}/crew/unassign/{id}")

    def get_game_config(self):
        return self.get("/configuration")

    def compute_ship_wages(self, opranks, pilot_rank=None):
        if pilot_rank is None:
            return self.post("/calc/wages/ship", body={"operator_ranks": opranks})
        else:
            return self.post("/calc/wages/ship", body={
                "operator_ranks": opranks,
                "pilot_rank": pilot_rank,
            })

    def get_player_history(self):
        return self.get("/player/history")

    def reassign_operator(self, ship_id, opid, mid):
        return self.post(f"/ship/{ship_id}/reassign/{opid}/{mid}")
