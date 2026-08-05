#!/usr/bin/env python3
"""Rebuild a core that cannot be called where it sits.

A scripted `nn.LSTM` held as a submodule of a TorchScript archive has no callable
`forward` — TorchScript lowers recurrent layers into the parent's graph rather than
leaving a method on the child. So the layer cannot be converted in place, and its
parents are blocked for unrelated reasons: a boolean mask in the awake-HR profile
selector, a packed-sequence length read in the popsicle ovulation head.

The weights are all there and all named. Rebuilding a plain `nn.LSTM` with the same
configuration and loading them gives an equivalent, callable module. `load_state_dict`
with `strict=True` is what makes it equivalent rather than approximately so: every
weight the archive holds must land in a slot the rebuilt layer expects, and any
mismatch in size, direction count or layer count fails there instead of silently
producing a differently-shaped network.

Where the layer's parent is blocked only by that one child — as the popsicle heads are —
the rebuild takes in the parent's other layers too, so the head ships as one model rather
than as an encoder and a stranded tail.
"""
import torch


def get_core(model, path):
    node = model
    for part in [p for p in (path or "").split(".") if p]:
        node = getattr(node, part)
    return node


class SequenceOutput(torch.nn.Module):
    """Returns only the output sequence.

    An `nn.LSTM` also returns its final hidden and cell state. Those are not part of any
    contract here — every consumer reads the per-step output — and a converter handed a
    nested tuple has to be told how to flatten it, so the wrapper drops them.
    """

    def __init__(self, layer):
        super().__init__()
        self.layer = layer

    def forward(self, sequence):
        output, _state = self.layer(sequence)
        return output


class PopsicleRunner(torch.nn.Module):
    """The whole popsicle head: recurrent encoder, scalar branch, and the layer joining them.

    Only the recurrent layer needed rebuilding; the two linear heads are callable where they
    sit. They are rebuilt with it anyway, because splitting a 161-parameter tail off an
    84,224-parameter encoder buys nothing and costs a second artefact, a second contract and a
    second place for the two to drift apart.

    The detection variant's wrapper packs the sequence before the encoder and unpacks it after.
    At batch one and a full-length sequence — which is what a fixed-shape contract is — packing
    is the identity, so the plain encoder computes the same thing. Its only other effect is to
    zero the outputs past `ts_lengths`, and Rust reads only the first `ts_lengths` steps.
    """

    def __init__(self, lstm, fc_scalar, fc_combined, squash):
        super().__init__()
        self.lstm = lstm
        self.fc_scalar = fc_scalar
        self.fc_combined = fc_combined
        self.squash = squash

    def forward(self, time_series, scalars):
        encoded, _state = self.lstm(time_series)
        scalar_branch = torch.relu(self.fc_scalar(scalars))
        combined = self.fc_combined(torch.cat([encoded, scalar_branch], 2))
        return torch.sigmoid(combined) if self.squash else combined


def _load(module, scripted, what):
    weights = {name: parameter.detach().clone() for name, parameter in scripted.named_parameters()}
    module.load_state_dict(weights, strict=True)
    module.eval()
    return module


def rebuild(model, core_path, kind, config):
    """Return a plain, callable module equivalent to the scripted core at `core_path`."""
    scripted = get_core(model, core_path)

    if kind == "lstm":
        layer = torch.nn.LSTM(batch_first=True, **config)
        return SequenceOutput(_load(layer, scripted, core_path)).eval()

    if kind == "popsicle_runner":
        settings = dict(config)
        squash = settings.pop("squash")
        lstm = torch.nn.LSTM(batch_first=True, **settings)
        _load(lstm, scripted.lstm, f"{core_path}.lstm")
        scalar_in, scalar_out = scripted.fc_scalar.weight.shape[1], scripted.fc_scalar.weight.shape[0]
        combined_in = scripted.fc_combined.weight.shape[1]
        fc_scalar = _load(torch.nn.Linear(scalar_in, scalar_out), scripted.fc_scalar, "fc_scalar")
        fc_combined = _load(torch.nn.Linear(combined_in, 1), scripted.fc_combined, "fc_combined")
        if combined_in != settings["hidden_size"] + scalar_out:
            raise ValueError(
                f"{core_path}: fc_combined takes {combined_in}, encoder and scalar branch "
                f"produce {settings['hidden_size']} + {scalar_out}"
            )
        return PopsicleRunner(lstm, fc_scalar, fc_combined, squash).eval()

    raise ValueError(f"no rebuild recipe for {kind!r}")
