# Validation contract

Writing /record requires either caller permission acme.write or independently checked acme.fallback. Denying both must prevent the write.
