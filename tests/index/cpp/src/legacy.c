typedef struct {
    int id;
} Job;

static int jobs_run;

int run_job(Job *job) {
    return job->id;
}
