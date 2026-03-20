from setuptools import setup

setup(
    name="openparlant",
    version="0.1.0",
    description="Official Python client for the OpenParlant Agent OS REST API",
    py_modules=["openparlant_sdk", "openparlant_client"],
    python_requires=">=3.8",
    classifiers=[
        "Programming Language :: Python :: 3",
        "License :: OSI Approved :: MIT License",
        "Operating System :: OS Independent",
    ],
)
