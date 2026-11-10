# everycli v1.1.1

This update makes automatic offers easier to configure and explains pricing
failures directly in the CLI and dashboard.

- **Select available models:** preview prices first, then choose only models
  with fresh, complete prices that pass your limits. Other models show their
  reasons separately. Changing limits requires a new check.
- **Clearer pricing errors:** see which model failed and whether its price check
  timed out, was rate-limited, lacked permission or returned a placeholder price.
- **More time for coordinator requests:** automatic-offer operations use a bounded
  30-second timeout. An uncertain policy save is never automatically repeated.
- **VEED pricing support:** automatic offers recognize input-second pricing for
  the reviewed VEED Dubbing and VEED Dubbing Pro models.

## Upgrade

```sh
everycli update
everycli ui
```

If you use automatic offers, also install the matching signed worker:

```sh
everycli worker-update
```

Then open **Models → Automatic offers**, enter your limits, choose **Preview
prices**, select available models and **Review and enable** if you want to save
a policy. Prices can change, so enabling checks them again.

Existing VMs, manual offers, discounts and saved policies are retained. Updating
does not opt you into automatic offers. If a policy save has an uncertain result,
check `everycli auto-offers` before making another change.
