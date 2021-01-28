from kubernetes import client, config
import random
import sys
from pprint import pprint
from faker import Faker
fake = Faker()

nmooses = int(sys.argv[1])
moose_names = set([])

while len(moose_names) < nmooses:
    name = fake.unique.first_name_nonbinary()
    if name[0] == "M":
        moose_names.add(name)

config.load_kube_config()

api = client.CustomObjectsApi()


for name in moose_names:
    antlers = fake.boolean()
    height = random.gauss(1.7, 0.1)
    if antlers:
        weight = random.gauss(540, 53)
    else:
        weight = random.gauss(345, 48)

    moose = {
        "apiVersion": "animals.com/v1",
        "kind": "Moose",
        "metadata": {
            "name": name.lower(),
            "labels": {
                "nps.gov/park": "glacier"
            }
        },
        "spec": {
            "height": height,
            "weight": weight,
            "antlers": antlers
        }
    }

    pprint(api.create_namespaced_custom_object(
        group="animals.com",
        version="v1",
        namespace="default",
        plural="mooses",
        body=moose,
    ))
