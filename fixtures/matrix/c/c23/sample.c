/* C23: typeof, nullptr (representative; not the full stdlib). */
void sample(void) {
    typeof(int) x = 0;
    void *p = nullptr;
    (void)x;
    (void)p;
}
