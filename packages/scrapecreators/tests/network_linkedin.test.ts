import { label } from "submilli:test";
import secrets from "submilli:secrets";
import { linkedin } from "@submilli/scrapecreators";
function main(): void {
    const key = secrets.get("SCRAPECREATORS_API_KEY");
    if (key === undefined || key.trim().length === 0) {
        label("skip: SCRAPECREATORS_API_KEY is not bound");
        return;
    }
    label("live LinkedIn company retains identity");
    const company = linkedin.getCompany({ url: "https://www.linkedin.com/company/microsoft" }).data;
    assert(company.success && company.name !== undefined && company.name !== null, "LinkedIn company name");
}
